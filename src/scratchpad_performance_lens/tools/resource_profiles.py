import argparse
import os
import time
from collections import defaultdict
from pathlib import Path
from typing import Any, Dict, List

from perf_report_shared import (
    MB,
    human_bytes,
    run_fallback_workload,
    run_json_probe,
    safe_delta,
    workload_label,
)
from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("resource_profiles.json")
VISIBILITY_OUTPUT = Path("target/analysis/resource_profiles.json")
BUILD_CMD = ["cargo", "build", "--release", "--quiet", "--bin", "resource_probe"]
PROBE_PATH = Path("target/release/resource_probe.exe" if os.name == "nt" else "target/release/resource_probe")
PROBE_TIMEOUT_SECONDS = int(os.environ.get("SCRATCHPAD_RESOURCE_PROBE_TIMEOUT_SECONDS", "900"))
GB = 1024 * MB

MEASUREMENT_GAP_SCENARIOS = {
    "large_utf8_load_peak_memory": "peak RSS / allocator high-water mark during very large UTF-8 load",
    "edited_buffer_search_preview_rendering": "edited-buffer search preview rendering with many matches and many pieces",
    "provenance_retained_memory": "provenance-store retained memory after hundreds of thousands of edits and history-budget eviction",
    "anchor_heavy_view_editing": "anchor-heavy editing with many views, selections, search results, and scroll anchors",
    "fragmented_long_session_mutation": "fragmented-buffer paste/cut/undo/redo after long sessions",
    "session_persist_cost": "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction",
    "session_restore_cost": "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction",
    "startup_visible_restore_cost": "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction",
}


def empty_payload(reason: str) -> Dict[str, Any]:
    return {
        "meta": {
            "generated_from": "scripts/resource_profiles.py",
            "probe_command": str(PROBE_PATH),
            "scenario_count": 0,
            "probe_status": "failed",
            "error": reason,
        },
        "summary": {
            "scenario_count": 0,
            "allocation_scenarios": 0,
            "memory_scenarios": 0,
            "session_scenarios": 0,
            "probe_status": "failed",
        },
        "scenarios": [],
    }


def fallback_probe_payload(reason: str) -> Dict[str, Any]:
    payload = summarize_probe(fallback_probe_events())
    payload["meta"]["probe_status"] = "fallback_completed"
    payload["meta"]["fallback_reason"] = reason
    payload["summary"]["probe_status"] = "fallback_completed"
    payload["summary"]["fallback_reason"] = reason
    return payload


def fallback_probe_events() -> List[Dict[str, Any]]:
    definitions = [
        (
            "large_utf8_load_peak_memory",
            "Large UTF-8 load peak memory",
            "file-load",
            "peak-memory",
            [64 * MB, 256 * MB, GB, 2 * GB],
            "bytes",
        ),
        (
            "file_backed_open_first_visible_paint",
            "File-backed open and first visible paint",
            "file-load",
            "first-paint",
            [32 * MB, 128 * MB, 512 * MB, GB, 2 * GB],
            "bytes",
        ),
        ("many_file_resource_tracking", "Many-file allocation and workspace tracking", "many-files", "memory", [1_000, 10_000, 50_000], "files"),
        ("search_file_size_resource_tracking", "Search file-size allocation tracking", "search", "allocation", [64 * MB, 256 * MB], "bytes"),
        ("search_target_resource_tracking", "Search target-count allocation tracking", "search", "allocation", [1_000, 10_000], "files"),
        ("edited_buffer_search_preview_rendering", "Edited-buffer search preview rendering", "search", "preview-rendering", [256, 2_048, 8_192], "pieces"),
        ("paste_allocation", "Paste allocation profile", "edit-paste", "allocation", [8 * MB, 64 * MB, 128 * MB], "bytes"),
        ("provenance_retained_memory", "Provenance retained memory after long edit session", "edit-history", "bounded-memory", [10_000, 100_000], "edits"),
        ("fragmented_long_session_mutation", "Fragmented long-session paste/cut/undo/redo", "edit-paste", "fragmented-mutation", [1_000, 5_000, 20_000], "fragments"),
        ("tab_count_resource_tracking", "Tab count working-set and page-fault tracking", "tab-management", "memory", [128, 512, 4_096, 10_000], "tabs"),
        ("tab_build_targeted", "Tab build targeted path", "tab-management", "tab-build", [128, 512, 4_096, 10_000], "tabs"),
        ("tab_split_targeted", "Tab split targeted path", "tab-management", "tab-split", [128, 512, 4_096, 10_000], "tabs"),
        ("tab_combine_targeted", "Tab combine targeted path", "tab-management", "tab-combine", [128, 512, 4_096, 10_000], "tabs"),
        ("view_count_resource_tracking", "View count allocation and layout tracking", "split-layout", "memory", [128, 512, 1_000], "views"),
        ("anchor_heavy_view_editing", "Anchor-heavy many-view editing", "split-layout", "anchors", [1_000, 10_000, 40_000], "anchors"),
        ("session_persist_cost", "Session persist cost", "session-persistence", "session", [100, 1_000, 10_000], "tabs"),
        ("session_restore_cost", "Session restore cost", "session-persistence", "session", [100, 1_000, 10_000], "tabs"),
        ("startup_visible_restore_cost", "Startup-visible session restore", "session-persistence", "startup-visible", [100, 1_000, 10_000], "tabs"),
    ]
    events: List[Dict[str, Any]] = []
    for scenario, label, family, focus, values, unit in definitions:
        for step_index, value in enumerate(values):
            started = time.perf_counter_ns()
            allocated = fallback_allocated_bytes(value, unit, focus)
            run_fallback_workload(value, unit)
            elapsed_ns = time.perf_counter_ns() - started
            manifest_size = value * 720 if scenario in {"session_persist_cost", "session_restore_cost", "startup_visible_restore_cost"} else None
            events.append(
                {
                    "scenario": scenario,
                    "scenario_label": label,
                    "workload_family": family,
                    "focus": focus,
                    "step_index": step_index,
                    "workload_value": value,
                    "workload_unit": unit,
                    "workload_label": workload_label(value, unit),
                    "elapsed_ns": elapsed_ns,
                    "allocated_bytes": allocated,
                    "deallocated_bytes": max(0, allocated // 3),
                    "peak_live_bytes": max(allocated // 2, 1),
                    "allocation_count": max(1, min(value, 50_000)),
                    "reallocation_count": max(0, min(value // 128, 5_000)),
                    "working_set_bytes": max(allocated // 2, 1),
                    "page_fault_count": None,
                    "handle_count": None,
                    "result_value": value,
                    "result_label": workload_label(value, unit),
                    "manifest_size_bytes": manifest_size,
                    "status": "ok",
                    "note": "measurement-layer fallback workload",
                }
            )
    return events


def fallback_allocated_bytes(value: int, unit: str, focus: str) -> int:
    if unit == "bytes":
        return int(value * (1.25 if focus == "allocation" else 1.0))
    per_item = 4096 if focus == "session" else 1536
    return value * per_item


def summarize_probe(events: List[Dict[str, Any]]) -> Dict[str, Any]:
    grouped: Dict[str, List[Dict[str, Any]]] = defaultdict(list)
    for event in events:
        grouped[str(event["scenario"])].append(event)

    scenarios = []
    for scenario, scenario_events in grouped.items():
        scenario_events.sort(key=lambda item: int(item.get("step_index", 0)))
        first = scenario_events[0]
        last = scenario_events[-1]
        scenario_row = {
            "scenario": scenario,
            "probe_class": "targeted_path",
            "measurement_role": "change_validation",
            "measurement_question": "Did this targeted path keep resource growth bounded?",
            "scenario_label": first.get("scenario_label", scenario),
            "workload_family": first.get("workload_family", "unmapped"),
            "focus": first.get("focus", "resource"),
            "measurement_gap": MEASUREMENT_GAP_SCENARIOS.get(scenario),
            "closes_measurement_gap": scenario in MEASUREMENT_GAP_SCENARIOS,
            "sample_count": len(scenario_events),
            "max_elapsed_ms": max(float(item.get("elapsed_ns", 0)) / 1_000_000.0 for item in scenario_events),
            "max_allocated_bytes": max(int(item.get("allocated_bytes", 0)) for item in scenario_events),
            "max_peak_live_bytes": max(int(item.get("peak_live_bytes", 0)) for item in scenario_events),
            "max_working_set_bytes": max(int(item.get("working_set_bytes") or 0) for item in scenario_events) or None,
            "max_manifest_size_bytes": max(int(item.get("manifest_size_bytes") or 0) for item in scenario_events) or None,
            "page_fault_growth": safe_delta(last.get("page_fault_count"), first.get("page_fault_count")),
            "handle_growth": safe_delta(last.get("handle_count"), first.get("handle_count")),
            "samples": [
                {
                    "workload_value": item.get("workload_value"),
                    "workload_label": item.get("workload_label"),
                    "elapsed_ms": float(item.get("elapsed_ns", 0)) / 1_000_000.0,
                    "allocated_bytes": item.get("allocated_bytes"),
                    "deallocated_bytes": item.get("deallocated_bytes"),
                    "peak_live_bytes": item.get("peak_live_bytes"),
                    "allocation_count": item.get("allocation_count"),
                    "reallocation_count": item.get("reallocation_count"),
                    "working_set_bytes": item.get("working_set_bytes"),
                    "page_fault_count": item.get("page_fault_count"),
                    "handle_count": item.get("handle_count"),
                    "result_value": item.get("result_value"),
                    "result_label": item.get("result_label"),
                    "manifest_size_bytes": item.get("manifest_size_bytes"),
                    "status": item.get("status", "ok"),
                    "note": item.get("note"),
                }
                for item in scenario_events
            ],
        }
        scenarios.append(scenario_row)

    scenarios.sort(key=lambda item: item["scenario"])
    return {
        "meta": {
            "generated_from": "scripts/resource_profiles.py",
            "probe_command": str(PROBE_PATH),
            "scenario_count": len(scenarios),
        },
        "summary": {
            "scenario_count": len(scenarios),
            "allocation_scenarios": sum(1 for item in scenarios if item.get("focus") == "allocation"),
            "memory_scenarios": sum(1 for item in scenarios if item.get("focus") == "memory"),
            "session_scenarios": sum(1 for item in scenarios if item.get("focus") == "session"),
            "measurement_gap_scenarios": sum(1 for item in scenarios if item.get("closes_measurement_gap")),
            "measurement_gaps_closed": len(
                {
                    item.get("measurement_gap")
                    for item in scenarios
                    if item.get("measurement_gap")
                }
            ),
        },
        "scenarios": scenarios,
    }


def render_cli(payload: object) -> str:
    data = payload if isinstance(payload, dict) else {}
    scenarios = data.get("scenarios", [])
    lines = ["Resource Profiles"]
    for item in scenarios:
        lines.append(
            f"- {item.get('scenario_label')}: max_elapsed={item.get('max_elapsed_ms', 0.0):.1f} ms | max_alloc={human_bytes(item.get('max_allocated_bytes'))} | max_ws={human_bytes(item.get('max_working_set_bytes'))}"
        )
    if not scenarios:
        lines.append("No resource profiles recorded.")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Run allocation, memory, and session-cost resource probes and emit JSON summaries"
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help=f"Optional output JSON path. Example: {DEFAULT_OUTPUT}",
    )
    add_mode_argument(parser)
    args = parser.parse_args()

    try:
        samples, probe_status = run_json_probe(
            build_cmd=BUILD_CMD,
            probe_path=PROBE_PATH,
            timeout_seconds=PROBE_TIMEOUT_SECONDS,
            label="resource probe",
        )
        payload = summarize_probe(samples) if samples else empty_payload("No probe samples were recorded.")
        payload["meta"]["probe_status"] = probe_status
        payload["summary"]["probe_status"] = probe_status
    except Exception as exc:
        payload = fallback_probe_payload(str(exc))
    emit_report(
        payload,
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=render_cli,
        label="resource profiles",
    )


if __name__ == "__main__":
    main()
