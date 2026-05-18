from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import runpy
import sys
from typing import Sequence

from .config import LensConfig


TOOLS_DIR = Path(__file__).resolve().parent / "tools"

MEASURE_TOOLS = {
    "slowspots": ("slowspots", "slowspots.json"),
    "frame-metrics": ("frame_metrics", "frame_metrics.json"),
    "search": ("search_speed", "search_speed.json"),
    "capacity": ("capacity_report", "capacity_report.json"),
    "resources": ("resource_profiles", "resource_profiles.json"),
    "flamegraphs": ("generate_flamegraphs", "flamegraphs.json"),
    "speed-report": ("speed_efficiency_report", "speed_efficiency_report.json"),
    "performance-review": ("performance_review", "performance_review.json"),
    "project-code": ("project_code_metrics", "project_code_metrics.json"),
}

STANDARD_RUN_ORDER = [
    "slowspots",
    "frame-metrics",
    "search",
    "capacity",
    "resources",
    "speed-report",
    "performance-review",
    "project-code",
]


def run_tool(module_name: str, argv: Sequence[str], config: LensConfig) -> None:
    old_argv = sys.argv[:]
    old_cwd = Path.cwd()
    old_path = sys.path[:]
    old_env = {
        "SPLENS_OUTPUT_DIR": os.environ.get("SPLENS_OUTPUT_DIR"),
        "SPLENS_PROJECT_NAME": os.environ.get("SPLENS_PROJECT_NAME"),
    }
    try:
        os.chdir(config.project_root)
        sys.path.insert(0, str(TOOLS_DIR))
        os.environ["SPLENS_OUTPUT_DIR"] = str(config.output_dir)
        os.environ["SPLENS_PROJECT_NAME"] = config.project_name
        sys.argv = [module_name, *argv]
        runpy.run_module(module_name, run_name="__main__")
    finally:
        sys.argv = old_argv
        sys.path[:] = old_path
        os.chdir(old_cwd)
        for key, value in old_env.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def selected_tools(tool: str) -> list[str]:
    if tool == "all":
        return list(STANDARD_RUN_ORDER)
    if tool == "all-with-flamegraphs":
        return [*STANDARD_RUN_ORDER, "flamegraphs"]
    return [tool]


def measure(args: argparse.Namespace) -> None:
    config = LensConfig.load(args.config)
    config.output_dir.mkdir(parents=True, exist_ok=True)
    for tool in selected_tools(args.tool):
        module_name, file_name = MEASURE_TOOLS[tool]
        argv = ["--mode", "visibility", "--output", str(config.output_dir / file_name)]
        if tool == "slowspots" and args.skip_bench:
            argv.append("--skip-bench")
        if tool == "search" and args.skip_bench:
            argv.append("--skip-bench")
        if tool in {"slowspots", "search"} and args.fail_on_slow:
            argv.append("--fail-on-slow")
        if tool == "flamegraphs" and args.index_only:
            argv.append("--index-only")
        run_tool(module_name, argv, config)


def catalog(args: argparse.Namespace) -> None:
    config = LensConfig.load(args.config)
    tasks = [
        {
            "id": f"performance.{tool}",
            "category": "performance",
            "title": tool.replace("-", " ").title(),
            "output_artifacts": [str(config.output_dir / file_name)],
        }
        for tool, (_, file_name) in MEASURE_TOOLS.items()
    ]
    tasks.append(
        {
            "id": "telemetry.app-package",
            "category": "telemetry",
            "title": "App Package",
            "output_artifacts": [],
        }
    )
    print(json.dumps({"version": 1, "project_name": config.project_name, "tasks": tasks}, indent=2))


def telemetry(args: argparse.Namespace) -> None:
    config = LensConfig.load(args.config)
    old_cwd = Path.cwd()
    old_path = sys.path[:]
    try:
        os.chdir(config.project_root)
        sys.path.insert(0, str(TOOLS_DIR))
        from app_package import app_package_payload

        print(json.dumps(app_package_payload(), indent=2))
    finally:
        sys.path[:] = old_path
        os.chdir(old_cwd)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Scratchpad performance measurement JSON producers")
    subcommands = parser.add_subparsers(dest="command", required=True)

    measure_parser = subcommands.add_parser("measure", help="Run measurement producers")
    measure_parser.add_argument(
        "tool",
        nargs="?",
        default="all",
        choices=["all", "all-with-flamegraphs", *MEASURE_TOOLS.keys()],
    )
    measure_parser.add_argument("--config", type=Path, default=None)
    measure_parser.add_argument("--skip-bench", action="store_true")
    measure_parser.add_argument("--fail-on-slow", action="store_true")
    measure_parser.add_argument("--index-only", action="store_true")
    measure_parser.set_defaults(func=measure)

    catalog_parser = subcommands.add_parser("catalog", help="Print producer catalog")
    catalog_parser.add_argument("--config", type=Path, default=None)
    catalog_parser.set_defaults(func=catalog)

    telemetry_parser = subcommands.add_parser("telemetry", help="Print app-package telemetry JSON")
    telemetry_parser.add_argument("--config", type=Path, default=None)
    telemetry_parser.set_defaults(func=telemetry)

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
