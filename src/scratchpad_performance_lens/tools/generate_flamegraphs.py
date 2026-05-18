import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import List, Optional

from perf_report_shared import flamegraph_configs
from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("flamegraphs.json")
VISIBILITY_OUTPUT = Path("target/analysis/flamegraphs.json")
FLAMEGRAPH_DIR = Path("target/analysis/flamegraphs")
STACKS_PATH = Path("cargo-flamegraph.stacks")
MIN_FREE_BYTES = 20 * 1024 * 1024 * 1024

BENCHMARKS = flamegraph_configs()


class FlamegraphGenerator:
    def __init__(self, output_dir: Path):
        self.output_dir = output_dir

    def load_existing_results(self, index_path: Optional[Path]) -> List[dict]:
        if index_path is None or not index_path.exists():
            return []

        try:
            payload = json.loads(index_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return []

        return payload if isinstance(payload, list) else []

    def check_tool(self) -> bool:
        try:
            subprocess.run(
                ["cargo", "flamegraph", "--version"],
                capture_output=True,
                check=True,
                text=True,
            )
            return True
        except (subprocess.CalledProcessError, FileNotFoundError):
            return False

    def free_bytes(self) -> int:
        return shutil.disk_usage(Path.cwd()).free

    def has_enough_disk_space(self) -> bool:
        return self.free_bytes() >= MIN_FREE_BYTES

    def cleanup_stack_dump(self) -> None:
        try:
            if STACKS_PATH.exists():
                STACKS_PATH.unlink()
        except OSError as exc:
            print(
                f"Warning: Could not remove leftover stack dump '{STACKS_PATH}': {exc}",
                file=sys.stderr,
            )

    def warn_low_disk_space(self) -> None:
        free_gb = self.free_bytes() / (1024**3)
        required_gb = MIN_FREE_BYTES / (1024**3)
        print(
            (
                "Warning: Flamegraph generation skipped because disk space is low "
                f"({free_gb:.1f} GB free, {required_gb:.0f} GB required minimum)."
            ),
            file=sys.stderr,
        )

    def existing_result_map(self, existing_results: List[dict]) -> dict[str, dict]:
        return {
            str(item.get("id")): item
            for item in existing_results
            if isinstance(item, dict) and item.get("id")
        }

    def result_from_config(
        self,
        config: dict,
        existing: Optional[dict] = None,
        *,
        available: Optional[bool] = None,
        issue: Optional[str] = None,
    ) -> dict:
        svg_path = self.output_dir / f"{config['id']}.svg"
        relative_path = f"flamegraphs/{config['id']}.svg"
        existing_available = svg_path.exists()
        resolved_available = existing_available if available is None else available
        item = {
            "id": config["id"],
            "name": config["name"],
            "path": relative_path,
            "type": "svg" if resolved_available else "missing",
            "available": resolved_available,
            "benchmark_keys": list(config.get("benchmark_keys", [])),
            "workload_families": list(config.get("workload_families", [])),
            "coverage_role": config.get("coverage_role", "report-driven"),
            "resource_focus": config.get("resource_focus", "cpu"),
            "description": config.get("description", ""),
        }
        if issue and not resolved_available:
            item["issue"] = issue
        return item

    def materialize_index(
        self,
        existing_results: List[dict],
        updates: Optional[dict[str, dict]] = None,
        global_issue: Optional[str] = None,
    ) -> List[dict]:
        existing_by_id = self.existing_result_map(existing_results)
        merged_updates = updates or {}
        results = []
        for config in BENCHMARKS:
            item = merged_updates.get(config["id"])
            if item is None:
                item = self.result_from_config(
                    config,
                    existing_by_id.get(config["id"]),
                    issue=global_issue,
                )
            results.append(item)
        return results

    def is_disk_full_error(self, output: str) -> bool:
        lowered = output.lower()
        return (
            "storagefull" in lowered
            or "disk is full" in lowered
            or "no space on device" in lowered
            or "there is not enough space on the disk" in lowered
        )

    def generate(
        self,
        skip_if_missing: bool = True,
        existing_index_path: Optional[Path] = None,
        index_only: bool = False,
    ) -> List[dict]:
        existing_results = self.load_existing_results(existing_index_path)
        results_by_id: dict[str, dict] = {}

        if index_only:
            return self.materialize_index(existing_results)

        if not self.check_tool():
            print("Warning: `cargo-flamegraph` not found.", file=sys.stderr)
            return self.materialize_index(
                existing_results,
                global_issue="cargo-flamegraph is not installed; coverage is indexed but new SVGs were not generated.",
            )

        self.cleanup_stack_dump()

        if not self.has_enough_disk_space():
            self.warn_low_disk_space()
            return self.materialize_index(
                existing_results,
                global_issue="Disk space was below the minimum required for flamegraph generation.",
            )

        self.output_dir.mkdir(parents=True, exist_ok=True)

        for config in BENCHMARKS:
            svg_path = self.output_dir / f"{config['id']}.svg"
            print(f"Generating flamegraph for {config['name']}...", file=sys.stderr)

            if not self.has_enough_disk_space():
                self.warn_low_disk_space()
                return self.materialize_index(
                    existing_results,
                    updates=results_by_id,
                    global_issue="Disk space fell below the minimum during flamegraph generation.",
                )

            self.cleanup_stack_dump()

            cmd = [
                "cargo",
                "flamegraph",
                "--dev",
                "-o",
                str(svg_path),
            ]
            cmd.extend(config.get("cargo_args", []))
            program_args = config.get("program_args", [])
            if program_args:
                cmd.append("--")
                cmd.extend(program_args)

            try:
                # We don't use check=True here because we want to capture the 
                # NotAnAdmin error specifically on Windows.
                process = subprocess.run(cmd, capture_output=True, text=True)
                command_output = "\n".join(
                    part.strip() for part in [process.stderr, process.stdout] if part and part.strip()
                )
                
                if process.returncode == 0:
                    self.cleanup_stack_dump()
                    results_by_id[config["id"]] = self.result_from_config(
                        config,
                        available=True,
                    )
                else:
                    self.cleanup_stack_dump()
                    error_msg = command_output
                    if "NotAnAdmin" in error_msg:
                        print(
                            "Warning: Flamegraph generation requires admin privileges - new flamegraphs will not be generated.",
                            file=sys.stderr,
                        )
                        results_by_id[config["id"]] = self.result_from_config(
                            config,
                            issue="Administrator privileges are required to generate a fresh SVG on Windows.",
                        )
                        return self.materialize_index(existing_results, updates=results_by_id)
                    elif self.is_disk_full_error(error_msg):
                        free_gb = self.free_bytes() / (1024**3)
                        print(
                            (
                                "Warning: Flamegraph generation stopped because the disk filled up "
                                f"during '{config['name']}' ({free_gb:.1f} GB free after cleanup)."
                            ),
                            file=sys.stderr,
                        )
                        results_by_id[config["id"]] = self.result_from_config(
                            config,
                            issue="Generation stopped because the disk filled during this profile run.",
                        )
                        return self.materialize_index(existing_results, updates=results_by_id)
                    else:
                        print(f"Error: {config['id']} failed: {error_msg}", file=sys.stderr)
                        results_by_id[config["id"]] = self.result_from_config(
                            config,
                            issue=f"Generation failed: {error_msg}",
                        )
            except Exception as e:
                self.cleanup_stack_dump()
                print(f"Unexpected error for {config['id']}: {e}", file=sys.stderr)
                results_by_id[config["id"]] = self.result_from_config(
                    config,
                    issue=f"Unexpected generation error: {e}",
                )

        return self.materialize_index(existing_results, updates=results_by_id)


def main():
    parser = argparse.ArgumentParser(description="Generate flamegraphs for benchmarks.")
    parser.add_argument("--output", type=Path, help="Path to write the index JSON.")
    parser.add_argument(
        "--index-only",
        action="store_true",
        help="Refresh flamegraphs.json from existing SVGs without running cargo-flamegraph.",
    )
    add_mode_argument(parser, default="visibility")

    args = parser.parse_args()

    generator = FlamegraphGenerator(FLAMEGRAPH_DIR)
    resolved_output = (
        VISIBILITY_OUTPUT if args.mode == "visibility" and args.output is None else args.output
    )
    results = generator.generate(
        skip_if_missing=(args.mode != "cli"),
        existing_index_path=resolved_output,
        index_only=args.index_only,
    )

    def cli_renderer(data):
        if not data:
            return "No flamegraphs generated."
        lines = ["Flamegraph Results:"]
        for item in data:
            if not item.get("available"):
                lines.append(
                    f"  [ ] {item['name']} | {item.get('coverage_role', 'report-driven')} | {item.get('issue', 'not generated')}"
                )
            else:
                lines.append(
                    f"  [x] {item['name']}: {item['path']} | benchmarks={', '.join(item.get('benchmark_keys', [])) or '-'}"
                )
        return "\n".join(lines)

    emit_report(
        results,
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=cli_renderer,
        label="flamegraph index",
    )


if __name__ == "__main__":
    main()
