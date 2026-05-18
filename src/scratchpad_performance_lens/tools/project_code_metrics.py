import argparse
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple

from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("project_code_metrics.json")
VISIBILITY_OUTPUT = Path("target/analysis/project_code_metrics.json")

CODE_EXTENSIONS = {".rs"}

EXCLUDED_PARTS = {
    ".git",
    ".venv",
    "target",
    "target_test",
    "target-codex",
    "assets",
    "docs",
    "fonts",
    "log",
}

TEST_PATH_RE = re.compile(
    r"(^|[\\/])(tests?|benches|testdata|fixtures)([\\/]|$)|(^|[\\/])tests\.rs$|(_test|_tests|test_).*\.rs$"
)


@dataclass
class CodeCounts:
    application: int = 0
    test: int = 0
    other: int = 0

    @property
    def total(self) -> int:
        return self.application + self.test + self.other

    def to_dict(self) -> Dict[str, int]:
        return {
            "application": self.application,
            "test": self.test,
            "other": self.other,
            "total": self.total,
        }


def git(args: List[str]) -> str:
    result = subprocess.run(
        ["git", *args],
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return result.stdout


def tracked_code_paths(ref: str) -> List[str]:
    output = git(["ls-tree", "-r", "--name-only", ref])
    paths: List[str] = []
    for raw in output.splitlines():
        path = raw.strip()
        if not path:
            continue
        parts = set(Path(path).parts)
        if parts & EXCLUDED_PARTS:
            continue
        if Path(path).suffix.lower() in CODE_EXTENSIONS:
            paths.append(path)
    return paths


def read_file_at_ref(ref: str, path: str) -> Optional[str]:
    try:
        return git(["show", f"{ref}:{path}"])
    except subprocess.CalledProcessError:
        return None


def rust_test_line_mask(source: str) -> List[bool]:
    lines = source.splitlines()
    mask = [False] * len(lines)
    pending_cfg_test = False
    stack: List[Tuple[int, bool]] = []

    for index, line in enumerate(lines):
        stripped = line.strip()
        inherited = stack[-1][1] if stack else False
        line_is_test = inherited or pending_cfg_test
        if "#[cfg(test)]" in stripped or "cfg_attr(test" in stripped:
            pending_cfg_test = True
            line_is_test = True
        mask[index] = line_is_test

        opens = line.count("{")
        closes = line.count("}")
        if opens:
            for _ in range(opens):
                stack.append((1, inherited or pending_cfg_test))
            pending_cfg_test = False
        for _ in range(closes):
            if stack:
                stack.pop()
    return mask


def classify_path(path: str) -> str:
    normalized = path.replace("\\", "/")
    if TEST_PATH_RE.search(normalized):
        return "test"
    if normalized.startswith("src/") or normalized == "build.rs":
        return "application"
    return "other"


def count_path(ref: str, path: str) -> CodeCounts:
    content = read_file_at_ref(ref, path)
    if content is None:
        return CodeCounts()
    extension = Path(path).suffix.lower()
    category = classify_path(path)
    rust_test_mask = rust_test_line_mask(content) if extension == ".rs" and category == "application" else []
    counts = CodeCounts()

    for index, line in enumerate(content.splitlines()):
        if not line.strip():
            continue
        if rust_test_mask and index < len(rust_test_mask) and rust_test_mask[index]:
            counts.test += 1
        elif category == "application":
            counts.application += 1
        elif category == "test":
            counts.test += 1
        else:
            counts.other += 1
    return counts


def count_ref(ref: str) -> CodeCounts:
    totals = CodeCounts()
    for path in tracked_code_paths(ref):
        counts = count_path(ref, path)
        totals.application += counts.application
        totals.test += counts.test
        totals.other += counts.other
    return totals


def remote_ref() -> str:
    try:
        upstream = git(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).strip()
        if upstream:
            return upstream
    except subprocess.CalledProcessError:
        pass
    return "HEAD"


def commit_delta(sha: str) -> CodeCounts:
    totals = CodeCounts()
    try:
        output = git(["show", "--first-parent", "--format=", "--numstat", sha])
    except subprocess.CalledProcessError:
        return totals
    for line in output.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        added_text, deleted_text, path = parts[0], parts[1], parts[2]
        if not added_text.isdigit() or not deleted_text.isdigit():
            continue
        if Path(path).suffix.lower() not in CODE_EXTENSIONS:
            continue
        if set(Path(path).parts) & EXCLUDED_PARTS:
            continue
        net = int(added_text) - int(deleted_text)
        category = classify_path(path)
        if category == "application":
            totals.application += net
        elif category == "test":
            totals.test += net
        else:
            totals.other += net
    return totals


def subtract_counts(value: CodeCounts, delta: CodeCounts) -> CodeCounts:
    return CodeCounts(
        application=max(0, value.application - delta.application),
        test=max(0, value.test - delta.test),
        other=max(0, value.other - delta.other),
    )


def commit_history(ref: str, limit: int, current: CodeCounts) -> List[Dict[str, object]]:
    output = git(["log", "--first-parent", f"--max-count={limit}", "--date=iso-strict", "--pretty=format:%H%x09%cI%x09%s", ref])
    raw_commits = []
    for line in output.splitlines():
        parts = line.split("\t", 2)
        if len(parts) != 3:
            continue
        sha, committed_at, subject = parts
        raw_commits.append((sha, committed_at, subject))

    counts_by_sha: Dict[str, CodeCounts] = {}
    running = current
    for sha, _, _ in raw_commits:
        counts_by_sha[sha] = running
        running = subtract_counts(running, commit_delta(sha))

    commits = []
    for sha, committed_at, subject in reversed(raw_commits):
        counts = counts_by_sha[sha]
        commits.append(
            {
                "sha": sha,
                "short_sha": sha[:8],
                "date": committed_at,
                "subject": subject,
                "lines": counts.to_dict(),
            }
        )
    return commits


def build_payload(limit: int) -> Dict[str, object]:
    ref = remote_ref()
    latest_sha = git(["rev-parse", ref]).strip()
    latest_subject = git(["log", "-1", "--pretty=%s", ref]).strip()
    latest_date = git(["log", "-1", "--date=iso-strict", "--pretty=%cI", ref]).strip()
    current = count_ref(ref)
    return {
        "version": 1,
            "source": "rust_git_first_parent_history",
        "ref": ref,
        "latest_push": {
            "sha": latest_sha,
            "short_sha": latest_sha[:8],
            "date": latest_date,
            "subject": latest_subject,
        },
        "current": current.to_dict(),
        "history": commit_history(ref, limit, current),
    }


def render_cli(payload: object) -> str:
    data = payload if isinstance(payload, dict) else {}
    current = data.get("current", {}) if isinstance(data.get("current"), dict) else {}
    latest = data.get("latest_push", {}) if isinstance(data.get("latest_push"), dict) else {}
    return "\n".join(
        [
            "Project Code Metrics",
            f"- Ref: {data.get('ref', '-')}",
            f"- Latest GitHub push: {latest.get('short_sha', '-')} {latest.get('date', '-')}",
            f"- Application code: {current.get('application', 0)}",
            f"- Test code: {current.get('test', 0)}",
            f"- Other code: {current.get('other', 0)}",
            f"- Total code: {current.get('total', 0)}",
        ]
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Emit project code line split and git history data")
    parser.add_argument("--output", type=Path, default=None)
    parser.add_argument("--history-limit", type=int, default=40)
    add_mode_argument(parser)
    args = parser.parse_args()
    emit_report(
        build_payload(max(1, args.history_limit)),
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=render_cli,
        label="project code metrics",
    )


if __name__ == "__main__":
    main()
