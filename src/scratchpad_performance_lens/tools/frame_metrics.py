import argparse
import json
import os
import subprocess
from pathlib import Path
from typing import Any, Dict

from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("frame_metrics.json")
VISIBILITY_OUTPUT = Path("target/analysis/frame_metrics.json")
BUILD_CMD = ["cargo", "build", "--release", "--quiet", "--bin", "frame_metrics"]
PROBE_PATH = Path(
    "target/release/frame_metrics.exe" if os.name == "nt" else "target/release/frame_metrics"
)


def empty_payload(reason: str) -> Dict[str, Any]:
    return {
        "meta": {
            "generated_from": "scripts/frame_metrics.py",
            "probe_status": "failed",
            "error": reason,
        },
        "scenarios": [],
    }


def build_report() -> Dict[str, Any]:
    build = subprocess.run(BUILD_CMD, capture_output=True, text=True)
    if build.returncode != 0:
        return empty_payload(build.stdout + build.stderr)
    probe = subprocess.run([str(PROBE_PATH)], capture_output=True, text=True)
    if probe.returncode != 0:
        return empty_payload(probe.stdout + probe.stderr)
    try:
        payload = json.loads(probe.stdout)
    except json.JSONDecodeError as exc:
        return empty_payload(f"frame metrics probe returned invalid JSON: {exc}")
    if isinstance(payload, dict):
        payload.setdefault("meta", {})
        payload["meta"]["generated_from"] = "scripts/frame_metrics.py"
        payload["meta"]["probe_status"] = "completed"
        return payload
    return empty_payload("frame metrics probe returned a non-object payload")


def render_cli(payload: object) -> str:
    data = payload if isinstance(payload, dict) else {}
    lines = ["Frame Metrics"]
    for scenario in data.get("scenarios", []):
        lines.append(
            f"- {scenario.get('scenario_label')}: "
            f"p95={scenario.get('p95_ms', 0):.2f} ms, "
            f"p99={scenario.get('p99_ms', 0):.2f} ms, "
            f"budget={scenario.get('budget_ms', 0):.2f} ms"
        )
        phases = sorted(
            scenario.get("phases", []),
            key=lambda phase: float(phase.get("mean_ms") or 0),
            reverse=True,
        )
        if phases:
            leader = phases[0]
            lines.append(
                f"  top phase: {leader.get('phase')} "
                f"{leader.get('mean_ms', 0):.2f} ms mean"
            )
    if not data.get("scenarios"):
        lines.append("No frame metrics were produced.")
    return "\n".join(lines)


def probe_failed(payload: object) -> bool:
    if not isinstance(payload, dict):
        return True
    meta = payload.get("meta")
    return isinstance(meta, dict) and meta.get("probe_status") == "failed"


def main() -> None:
    parser = argparse.ArgumentParser(description="Emit 120 Hz frame metrics")
    parser.add_argument("--output", type=Path, default=None)
    add_mode_argument(parser)
    args = parser.parse_args()
    payload = build_report()
    emit_report(
        payload,
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=render_cli,
        label="frame metrics",
    )
    if probe_failed(payload):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
