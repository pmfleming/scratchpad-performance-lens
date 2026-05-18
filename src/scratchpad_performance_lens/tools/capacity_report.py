import argparse
import os
import time
from collections import defaultdict
from pathlib import Path
from typing import Any, Dict, List, Optional

from perf_report_shared import (
    MB,
    GB,
    human_bytes,
    matching_flamegraph_ids,
    run_fallback_workload,
    run_json_probe,
    safe_delta,
    workload_label,
)
from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("capacity_report.json")
VISIBILITY_OUTPUT = Path("target/analysis/capacity_report.json")
BUILD_CMD = ["cargo", "build", "--release", "--quiet", "--bin", "capacity_probe"]
PROBE_PATH = Path("target/release/capacity_probe.exe" if os.name == "nt" else "target/release/capacity_probe")
PROBE_TIMEOUT_SECONDS = int(os.environ.get("SCRATCHPAD_CAPACITY_PROBE_TIMEOUT_SECONDS", "300"))
LARGE_FILE_CAPACITY_THRESHOLD_MS = 180.0

SCENARIO_CONFIG = {
    "file_size_ceiling": {
        "threshold_ms": LARGE_FILE_CAPACITY_THRESHOLD_MS,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": None,
        "memory_guidance": "Use allocation or working-set profiling before adding another CPU flamegraph.",
        "cpu_guidance": "If load or redraw dominates, compare against the file-load and scroll latency rows before adding a dedicated load profile.",
    },
    "text_layout_ceiling": {
        "threshold_ms": LARGE_FILE_CAPACITY_THRESHOLD_MS,
        "workload_family": "text-layout",
        "cpu_flamegraph_id": None,
        "measurement_question": "Can raw text layout stay bounded as submitted text grows?",
        "memory_guidance": "Inspect egui galley allocation and page-fault pressure before widening layout caches.",
        "cpu_guidance": "Compare against viewport extraction and scroll latency before adding another full-document layout profile.",
    },
    "tab_count_ceiling": {
        "threshold_ms": 140.0,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": "tab_operations_profile",
        "memory_guidance": "Inspect working-set growth and object retention across tab construction and combine operations.",
        "cpu_guidance": "Capture the tab operations flamegraph if the ceiling is CPU-bound.",
    },
    "many_file_count_ceiling": {
        "threshold_ms": 180.0,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": None,
        "memory_guidance": "Inspect buffer descriptor growth, file metadata retention, and restore costs at 10k+ files.",
        "cpu_guidance": "Add a many-file open or restore CPU profile if descriptor construction dominates.",
    },
    "search_file_size_ceiling": {
        "threshold_ms": 180.0,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": "search_capacity_profile",
        "memory_guidance": "Check match storage and scan-buffer allocation before adding more search workers.",
        "cpu_guidance": "Compare with search capacity and current/all-tabs search flamegraphs.",
    },
    "search_target_count_ceiling": {
        "threshold_ms": 180.0,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": "search_dispatch_profile",
        "memory_guidance": "Inspect target descriptor allocation and per-target result buffering.",
        "cpu_guidance": "Compare dispatch overhead with all-target search completion.",
    },
    "split_count_ceiling": {
        "threshold_ms": 120.0,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": "split_stress_profile",
        "memory_guidance": "Inspect pane-tree growth and allocation churn before chasing another CPU-only explanation.",
        "cpu_guidance": "Capture the split-stress flamegraph if split rebalance is the limiting path.",
    },
    "view_count_ceiling": {
        "threshold_ms": 120.0,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": "view_navigation_profile",
        "memory_guidance": "Inspect per-view state growth and pane-tree churn across duplicated views.",
        "cpu_guidance": "Capture view navigation and tile layout profiles at 1k+ views.",
    },
    "paste_size_ceiling": {
        "threshold_ms": 150.0,
        "workload_family": "capacity-stress",
        "cpu_flamegraph_id": "paste_stress_profile",
        "memory_guidance": "Check working-set growth and page-fault pressure around paste plus metadata refresh.",
        "cpu_guidance": "Capture the paste-stress flamegraph if mutation latency dominates without large memory growth.",
    },
}


def empty_payload(reason: str) -> Dict[str, Any]:
    return {
        "meta": {
            "generated_from": "scripts/capacity_report.py",
            "probe_command": str(PROBE_PATH),
            "scenario_count": 0,
            "probe_status": "failed",
            "error": reason,
        },
        "summary": {
            "scenario_count": 0,
            "ceilings_reached": 0,
            "memory_bound_scenarios": 0,
            "cpu_bound_scenarios": 0,
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
        ("file_size_ceiling", "File size ceiling sweep", [MB, 64 * MB, 256 * MB, GB], "bytes"),
        ("text_layout_ceiling", "Text Layout", [64 * 1024, MB, 4 * MB, 8 * MB, 16 * MB, 32 * MB, 64 * MB, 128 * MB], "bytes"),
        ("many_file_count_ceiling", "Many-file workspace ceiling sweep", [1_000, 10_000, 50_000], "files"),
        ("search_file_size_ceiling", "Search file-size ceiling sweep", [MB, 64 * MB, 256 * MB, GB], "bytes"),
        ("search_target_count_ceiling", "Search target-count ceiling sweep", [100, 1_000, 10_000], "files"),
        ("tab_count_ceiling", "Tab count ceiling sweep", [512, 4_096, 10_000], "tabs"),
        ("split_count_ceiling", "Split count ceiling sweep", [32, 128, 512, 1_000], "splits"),
        ("view_count_ceiling", "View count ceiling sweep", [128, 512, 1_000], "views"),
        ("paste_size_ceiling", "Paste size ceiling sweep", [8 * MB, 64 * MB, 128 * MB], "bytes"),
    ]
    events: List[Dict[str, Any]] = []
    for scenario, label, values, unit in definitions:
        for step_index, value in enumerate(values):
            started = time.perf_counter_ns()
            run_fallback_workload(value, unit)
            events.append(
                {
                    "scenario": scenario,
                    "scenario_label": label,
                    "workload_family": SCENARIO_CONFIG.get(scenario, {}).get(
                        "workload_family",
                        "capacity-stress",
                    ),
                    "step_index": step_index,
                    "workload_value": value,
                    "workload_unit": unit,
                    "workload_label": workload_label(value, unit),
                    "elapsed_ns": time.perf_counter_ns() - started,
                    "working_set_bytes": None,
                    "peak_working_set_bytes": None,
                    "page_fault_count": None,
                    "handle_count": None,
                    "status": "ok",
                    "note": "measurement-layer fallback workload",
                }
            )
    return events


def infer_limiting_resource(events: List[Dict[str, Any]]) -> str:
    first = events[0]
    last = events[-1]
    handle_growth = safe_delta(last.get("handle_count"), first.get("handle_count"))
    working_set_growth = safe_delta(
        last.get("working_set_bytes"),
        first.get("working_set_bytes"),
    )
    page_fault_growth = safe_delta(
        last.get("page_fault_count"),
        first.get("page_fault_count"),
    )

    if handle_growth is not None and handle_growth >= 256:
        return "os-handles"
    if page_fault_growth is not None and page_fault_growth >= 10_000:
        return "memory"
    if working_set_growth is not None and working_set_growth >= 128 * MB:
        return "memory"
    return "cpu"


def diagnosis_guidance(
    scenario: str,
    limiting_resource: str,
    matching_flamegraphs: List[str],
) -> List[str]:
    config = SCENARIO_CONFIG.get(scenario, {})
    guidance = []
    if limiting_resource == "cpu":
        cpu_guidance = config.get("cpu_guidance")
        if cpu_guidance:
            guidance.append(str(cpu_guidance))
        if matching_flamegraphs:
            guidance.append(
                f"Mapped CPU profile coverage: {', '.join(matching_flamegraphs)}."
            )
    elif limiting_resource == "memory":
        memory_guidance = config.get("memory_guidance")
        if memory_guidance:
            guidance.append(str(memory_guidance))
        guidance.append(
            "Prefer allocation, working-set, or page-fault diagnostics before adding another CPU flamegraph."
        )
    else:
        guidance.append(
            "Inspect handle counts, temp files, and other OS limits during the next stress run."
        )
    guidance.append(
        "Use the USE checklist: utilization, saturation, and errors for CPU, memory, I/O, and OS resources."
    )
    return guidance


def resource_checklist(
    limiting_resource: str,
    events: List[Dict[str, Any]],
) -> List[Dict[str, str]]:
    first = events[0]
    last = events[-1]
    working_set_growth = safe_delta(
        last.get("working_set_bytes"),
        first.get("working_set_bytes"),
    )
    page_fault_growth = safe_delta(
        last.get("page_fault_count"),
        first.get("page_fault_count"),
    )
    handle_growth = safe_delta(last.get("handle_count"), first.get("handle_count"))
    return [
        {
            "resource": "cpu",
            "status": "focus" if limiting_resource == "cpu" else "watch",
            "note": "Latency rose before another resource clearly saturated."
            if limiting_resource == "cpu"
            else "Capture a CPU flamegraph only if working-set growth stays modest.",
        },
        {
            "resource": "memory",
            "status": "focus" if limiting_resource == "memory" else "watch",
            "note": f"Working-set growth {human_bytes(working_set_growth)}; page-fault delta {page_fault_growth if page_fault_growth is not None else '-'}.",
        },
        {
            "resource": "i/o",
            "status": "not-measured",
            "note": "These sweeps are in-memory. Re-run with file-backed workloads if open/save ceilings are the concern.",
        },
        {
            "resource": "os-resources",
            "status": "focus" if limiting_resource == "os-handles" else "watch",
            "note": f"Handle growth {handle_growth if handle_growth is not None else '-'}.",
        },
    ]


def normalize_event(event: Dict[str, Any]) -> Dict[str, Any]:
    """Keep old artifacts readable after renaming layout bytes to Text Layout."""
    if event.get("scenario") != "layout_bytes_ceiling":
        return event
    normalized = dict(event)
    normalized["scenario"] = "text_layout_ceiling"
    normalized["scenario_label"] = "Text Layout"
    normalized["workload_family"] = "text-layout"
    return normalized


def summarize_probe(events: List[Dict[str, Any]]) -> Dict[str, Any]:
    grouped: Dict[str, List[Dict[str, Any]]] = defaultdict(list)
    for event in events:
        event = normalize_event(event)
        grouped[str(event["scenario"])].append(event)

    scenarios = []
    for scenario, scenario_events in grouped.items():
        scenario_events.sort(key=lambda item: int(item.get("step_index", 0)))
        config = SCENARIO_CONFIG.get(scenario, {})
        threshold_ms = float(config.get("threshold_ms", 100.0))
        first_failure = None
        last_success = None

        for event in scenario_events:
            elapsed_ms = float(event["elapsed_ns"]) / 1_000_000.0
            if event.get("status") != "ok":
                first_failure = event
                break
            if elapsed_ms > threshold_ms:
                first_failure = event
                break
            last_success = event

        limiting_resource = infer_limiting_resource(scenario_events)
        matching = []
        cpu_flamegraph_id = config.get("cpu_flamegraph_id")
        if isinstance(cpu_flamegraph_id, str) and cpu_flamegraph_id:
            matching.append(cpu_flamegraph_id)
        if not matching:
            matching = matching_flamegraph_ids(scenario)

        first = scenario_events[0]
        last = scenario_events[-1]
        peak_working_set = max(
            (item.get("peak_working_set_bytes") or item.get("working_set_bytes") or 0)
            for item in scenario_events
        ) or None
        scenario_row = {
            "scenario": scenario,
            "probe_class": "ceiling_health",
            "measurement_role": "promise_health",
            "measurement_question": config.get(
                "measurement_question",
                "Does this promise still pass as workload size increases?",
            ),
            "scenario_label": first.get("scenario_label", scenario),
            "workload_family": first.get("workload_family", config.get("workload_family", "capacity-stress")),
            "threshold_ms": threshold_ms,
            "failure_mode": (
                "panic"
                if first_failure and first_failure.get("status") != "ok"
                else "unusable_latency"
                if first_failure
                else "not_reached"
            ),
            "ceiling_reached": first_failure is not None,
            "last_successful_workload": last_success.get("workload_value") if last_success else None,
            "last_successful_label": last_success.get("workload_label") if last_success else None,
            "first_failure_workload": first_failure.get("workload_value") if first_failure else None,
            "first_failure_label": first_failure.get("workload_label") if first_failure else None,
            "peak_working_set_bytes": peak_working_set,
            "working_set_growth_bytes": safe_delta(
                last.get("working_set_bytes"),
                first.get("working_set_bytes"),
            ),
            "page_fault_growth": safe_delta(
                last.get("page_fault_count"),
                first.get("page_fault_count"),
            ),
            "handle_growth": safe_delta(
                last.get("handle_count"),
                first.get("handle_count"),
            ),
            "first_saturated_resource": limiting_resource,
            "suspected_limiting_resource": limiting_resource,
            "matching_flamegraphs": matching,
            "diagnosis_guidance": diagnosis_guidance(scenario, limiting_resource, matching),
            "resource_checklist": resource_checklist(limiting_resource, scenario_events),
            "samples": [
                {
                    "workload_value": item.get("workload_value"),
                    "workload_label": item.get("workload_label"),
                    "elapsed_ms": float(item["elapsed_ns"]) / 1_000_000.0,
                    "working_set_bytes": item.get("working_set_bytes"),
                    "page_fault_count": item.get("page_fault_count"),
                    "handle_count": item.get("handle_count"),
                    "status": item.get("status", "ok"),
                }
                for item in scenario_events
            ],
        }
        scenarios.append(scenario_row)

    scenarios.sort(key=lambda item: item["scenario"])
    ceilings_reached = sum(1 for item in scenarios if item["ceiling_reached"])
    memory_bound = sum(
        1 for item in scenarios if item["suspected_limiting_resource"] == "memory"
    )
    return {
        "meta": {
            "generated_from": "scripts/capacity_report.py",
            "probe_command": str(PROBE_PATH),
            "scenario_count": len(scenarios),
        },
        "summary": {
            "scenario_count": len(scenarios),
            "ceilings_reached": ceilings_reached,
            "memory_bound_scenarios": memory_bound,
            "cpu_bound_scenarios": sum(
                1 for item in scenarios if item["suspected_limiting_resource"] == "cpu"
            ),
        },
        "scenarios": scenarios,
    }


def render_cli(payload: object) -> str:
    data = payload if isinstance(payload, dict) else {}
    scenarios = data.get("scenarios", [])
    lines = ["Capacity Report"]
    for item in scenarios:
        ceiling = item.get("first_failure_label") or item.get("last_successful_label") or "-"
        lines.append(
            f"- {item.get('scenario_label', item.get('scenario'))}: ceiling={ceiling} | mode={item.get('failure_mode', '-')} | resource={item.get('suspected_limiting_resource', '-')}"
        )
    if not scenarios:
        lines.append("No capacity scenarios recorded.")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Run capacity threshold sweeps and emit ceiling, failure-mode, and resource-diagnosis JSON"
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
            label="capacity probe",
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
        label="capacity report",
    )


if __name__ == "__main__":
    main()
