import argparse
import json
from pathlib import Path
from typing import Any, Dict, List

from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("speed_efficiency_report.json")
VISIBILITY_OUTPUT = Path("target/analysis/speed_efficiency_report.json")
SLOWSPOTS_PATH = Path("target/analysis/slowspots.json")
SEARCH_SPEED_PATH = Path("target/analysis/search_speed.json")
FLAMEGRAPHS_PATH = Path("target/analysis/flamegraphs.json")
CAPACITY_PATH = Path("target/analysis/capacity_report.json")
RESOURCE_PROFILES_PATH = Path("target/analysis/resource_profiles.json")
FRAME_METRICS_PATH = Path("target/analysis/frame_metrics.json")

FAMILY_PRIORITY = {
    "search": 3,
    "search-dispatch": 3,
    "edit-paste": 3,
    "scroll": 3,
    "viewport": 3,
    "snapshot": 2,
    "split-layout": 2,
    "tab-management": 2,
    "session-persistence": 2,
    "anchor-maintenance": 1,
    "file-load": 2,
    "control-char-encoding": 1,
    "capacity-stress": 2,
    "unmapped": 0,
}

FAMILY_CEILING_SCENARIOS = {
    "file-load": "file_size_ceiling",
    "scroll": "file_size_ceiling",
    "viewport": "file_size_ceiling",
    "snapshot": "file_size_ceiling",
    "edit-paste": "paste_size_ceiling",
    "anchor-maintenance": "paste_size_ceiling",
    "tab-management": "tab_count_ceiling",
    "split-layout": "split_count_ceiling",
}


def load_json(path: Path, default: Any) -> Any:
    if not path.exists():
        return default
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default


def normalize_latency_row(item: Dict[str, Any], capacity_lookup: Dict[str, Dict[str, Any]]) -> Dict[str, Any]:
    family = str(item.get("workload_family", "unmapped"))
    ceiling = capacity_lookup.get(FAMILY_CEILING_SCENARIOS.get(family, ""), {})
    mean_ms = float(item.get("mean_ns", 0.0)) / 1_000_000.0
    threshold_ms = float(item.get("threshold_ms", 0.0))
    return {
        "scenario_id": item.get("name"),
        "probe_class": item.get("probe_class", "targeted_path"),
        "measurement_role": item.get("measurement_role", "change_validation"),
        "scenario_label": item.get("scenario_label", item.get("name")),
        "family": family,
        "mean_ms": mean_ms,
        "budget_ms": threshold_ms,
        "stability": item.get("stability", "stable"),
        "targets": item.get("targets", []),
        "matching_flamegraphs": item.get("matching_flamegraphs", []),
        "has_profile_coverage": bool(item.get("matching_flamegraphs")),
        "last_known_failure_ceiling": ceiling.get("last_successful_label") or ceiling.get("first_failure_label"),
        "suspected_limiting_resource": item.get("suspected_limiting_resource", "cpu"),
        "signals": item.get("signals", "nominal"),
        "over_budget": threshold_ms > 0 and mean_ms > threshold_ms,
    }


def normalize_capacity_row(item: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "scenario_id": item.get("scenario"),
        "probe_class": item.get("probe_class", "ceiling_health"),
        "measurement_role": item.get("measurement_role", "promise_health"),
        "scenario_label": item.get("scenario_label", item.get("scenario")),
        "family": item.get("workload_family", "capacity-stress"),
        "failure_mode": item.get("failure_mode", "not_reached"),
        "ceiling_reached": bool(item.get("ceiling_reached")),
        "last_successful_label": item.get("last_successful_label"),
        "first_failure_label": item.get("first_failure_label"),
        "matching_flamegraphs": item.get("matching_flamegraphs", []),
        "suspected_limiting_resource": item.get("suspected_limiting_resource", "cpu"),
        "peak_working_set_bytes": item.get("peak_working_set_bytes"),
        "working_set_growth_bytes": item.get("working_set_growth_bytes"),
        "diagnosis_guidance": item.get("diagnosis_guidance", []),
    }


def normalize_frame_row(item: Dict[str, Any]) -> Dict[str, Any]:
    scenario_id = str(item.get("scenario_id", "ui_render_frame_120hz"))
    budget_ms = float(item.get("budget_ms", 8.33))
    p99_budget_ms = float(item.get("p99_budget_ms", 12.0))
    mean_ms = float(item.get("mean_ms", 0.0))
    p95_ms = float(item.get("p95_ms", 0.0))
    p99_ms = float(item.get("p99_ms", 0.0))
    matching = ["ui_render_frame_profile"]
    phases = item.get("phases", [])
    top_phase = None
    if isinstance(phases, list) and phases:
        top_phase = max(phases, key=lambda phase: float(phase.get("mean_ms") or 0))
    signals = (
        f"p95 {p95_ms:.2f} ms vs {budget_ms:.2f} ms budget; "
        f"p99 {p99_ms:.2f} ms vs {p99_budget_ms:.2f} ms budget"
        if p95_ms
        else "frame metrics unavailable"
    )
    if top_phase:
        signals += (
            f"; top phase {top_phase.get('phase')} "
            f"{float(top_phase.get('mean_ms') or 0):.2f} ms mean"
        )
    return {
        "scenario_id": scenario_id,
        "probe_class": "targeted_path",
        "measurement_role": "change_validation",
        "scenario_label": item.get("scenario_label", scenario_id),
        "family": item.get("workload_family", "scroll"),
        "mean_ms": mean_ms,
        "p50_ms": item.get("p50_ms"),
        "p95_ms": p95_ms,
        "p99_ms": p99_ms,
        "max_ms": item.get("max_ms"),
        "budget_ms": budget_ms,
        "p99_budget_ms": p99_budget_ms,
        "stability": "stable",
        "targets": ["src/app/app_state/frame.rs", "src/app/ui"],
        "matching_flamegraphs": matching,
        "has_profile_coverage": True,
        "suspected_limiting_resource": "cpu",
        "signals": signals,
        "over_budget": bool(item.get("over_budget", p95_ms > budget_ms or p99_ms > p99_budget_ms)),
        "phases": phases,
    }


def latency_rank(row: Dict[str, Any]) -> float:
    budget_ms = float(row.get("budget_ms", 0.0))
    mean_ms = float(row.get("mean_ms", 0.0))
    overrun = (mean_ms / budget_ms) if budget_ms > 0 else mean_ms / 25.0
    stability_bonus = {
        "high-variance": 0.75,
        "watch": 0.25,
    }.get(str(row.get("stability", "stable")), 0.0)
    coverage_bonus = 0.0 if row.get("has_profile_coverage") else 0.5
    family_priority = FAMILY_PRIORITY.get(str(row.get("family", "unmapped")), 0)
    return (family_priority * 10.0) + overrun + stability_bonus + coverage_bonus


def capacity_rank(row: Dict[str, Any]) -> float:
    resource_bonus = 1.0 if row.get("suspected_limiting_resource") == "memory" else 0.5
    ceiling_bonus = 2.0 if row.get("ceiling_reached") else 0.0
    family_priority = FAMILY_PRIORITY.get(str(row.get("family", "capacity-stress")), 1)
    return (family_priority * 10.0) + ceiling_bonus + resource_bonus


def recommended_action(row: Dict[str, Any]) -> str:
    matching = row.get("matching_flamegraphs", [])
    if row.get("failure_mode") not in (None, "not_reached"):
        guidance = row.get("diagnosis_guidance", [])
        if guidance:
            return str(guidance[0])
    if matching:
        return f"Inspect {matching[0]} against the over-budget scenario."
    if row.get("family") == "search":
        return "Add or compare a search flamegraph before broad optimization work."
    return "Add diagnosis coverage before prioritizing an optimization." 


def flamegraph_coverage_rows(
    flamegraphs: List[Dict[str, Any]],
    latency_rows: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    by_benchmark = {row["scenario_id"].split("/", 1)[0]: row for row in latency_rows}
    rows = []
    for item in flamegraphs:
        covered = []
        for benchmark_key in item.get("benchmark_keys", []):
            if benchmark_key in by_benchmark:
                covered.append(by_benchmark[benchmark_key]["scenario_label"])
        rows.append(
            {
                "id": item.get("id"),
                "name": item.get("name"),
                "available": bool(item.get("available")),
                "coverage_role": item.get("coverage_role", "report-driven"),
                "benchmark_keys": item.get("benchmark_keys", []),
                "workload_families": item.get("workload_families", []),
                "covered_scenarios": covered,
                "issue": item.get("issue"),
            }
        )
    return rows


def build_triage(
    dispatch_rows: List[Dict[str, Any]],
    search_rows: List[Dict[str, Any]],
    editor_rows: List[Dict[str, Any]],
    tabs_rows: List[Dict[str, Any]],
    capacity_rows: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    triage = []
    for row in dispatch_rows + search_rows + editor_rows + tabs_rows:
        if not row.get("over_budget") and row.get("stability") == "stable":
            continue
        triage.append(
            {
                "scenario_id": row["scenario_id"],
                "scenario_label": row["scenario_label"],
                "family": row["family"],
                "reason": row["signals"],
                "suspected_limiting_resource": row["suspected_limiting_resource"],
                "recommended_action": recommended_action(row),
                "rank_score": latency_rank(row),
            }
        )

    for row in capacity_rows:
        if not row.get("ceiling_reached"):
            continue
        triage.append(
            {
                "scenario_id": row["scenario_id"],
                "scenario_label": row["scenario_label"],
                "family": row["family"],
                "reason": f"{row.get('failure_mode')} at {row.get('first_failure_label') or row.get('last_successful_label')}",
                "suspected_limiting_resource": row["suspected_limiting_resource"],
                "recommended_action": recommended_action(row),
                "rank_score": capacity_rank(row),
            }
        )

    triage.sort(key=lambda item: item["rank_score"], reverse=True)
    return triage[:5]


def methodology_notes() -> List[str]:
    return [
        "Broad Criterion slowspots remain the wide detector for general latency regressions.",
        "The dedicated search report remains the authoritative scaling view for search latency.",
        "Flamegraphs explain CPU hot paths; they do not replace benchmark budgets or capacity ceilings.",
        "Capacity sweeps stay out of the latency leaderboard and record the first unusable ceiling separately.",
        "Ceiling probes answer promise-health questions; targeted path probes validate whether a specific change worked.",
        "Resource profiles capture allocation-heavy, working-set, and session-cost scenarios that are not visible in CPU flamegraphs alone.",
        "The old slowspots p95 field is now treated as median absolute deviation dispersion, not a percentile.",
    ]


def render_cli(payload: object) -> str:
    data = payload if isinstance(payload, dict) else {}
    triage = data.get("triage", [])
    lines = ["Speed And Efficiency Report"]
    for index, item in enumerate(triage, start=1):
        lines.append(
            f"{index:>2}. {item.get('scenario_label')} | family={item.get('family')} | resource={item.get('suspected_limiting_resource')} | {item.get('recommended_action')}"
        )
    if not triage:
        lines.append("No investigation candidates were found.")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Emit a coordinated speed-efficiency report by combining latency, flamegraph, and capacity artifacts"
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help=f"Optional output JSON path. Example: {DEFAULT_OUTPUT}",
    )
    add_mode_argument(parser)
    args = parser.parse_args()

    slowspots = load_json(SLOWSPOTS_PATH, [])
    search_speed = load_json(SEARCH_SPEED_PATH, [])
    flamegraphs = load_json(FLAMEGRAPHS_PATH, [])
    capacity_report = load_json(CAPACITY_PATH, {"scenarios": []})
    resource_profiles = load_json(RESOURCE_PROFILES_PATH, {"scenarios": []})
    frame_metrics = load_json(FRAME_METRICS_PATH, {"scenarios": []})
    capacity_lookup = {
        item.get("scenario"): item for item in capacity_report.get("scenarios", [])
    }

    search_dispatch_rows = [
        normalize_latency_row(item, capacity_lookup)
        for item in search_speed
        if str(item.get("workload_family", "search")) == "search-dispatch"
    ]
    search_rows = [
        normalize_latency_row(item, capacity_lookup)
        for item in search_speed
        if str(item.get("workload_family", "search")) == "search"
    ]
    editor_rows = [
        normalize_latency_row(item, capacity_lookup)
        for item in slowspots
        if str(item.get("workload_family", "unmapped"))
        in {
            "file-load",
            "scroll",
            "viewport",
            "snapshot",
            "edit-paste",
            "anchor-maintenance",
            "session-persistence",
            "control-char-encoding",
        }
    ]
    editor_rows.extend(
        normalize_frame_row(item) for item in frame_metrics.get("scenarios", [])
    )
    tabs_rows = [
        normalize_latency_row(item, capacity_lookup)
        for item in slowspots
        if str(item.get("workload_family", "unmapped"))
        in {"tab-management", "split-layout"}
    ]
    capacity_rows = [
        normalize_capacity_row(item) for item in capacity_report.get("scenarios", [])
    ]
    flamegraph_rows = flamegraph_coverage_rows(
        flamegraphs,
        search_dispatch_rows + search_rows + editor_rows + tabs_rows,
    )
    triage = build_triage(
        search_dispatch_rows,
        search_rows,
        editor_rows,
        tabs_rows,
        capacity_rows,
    )

    latency_rows = search_dispatch_rows + search_rows + editor_rows + tabs_rows
    critical_count = sum(
        1
        for row in latency_rows
        if row.get("over_budget")
    ) + sum(1 for row in capacity_rows if row.get("ceiling_reached"))
    watch_count = sum(
        1
        for row in latency_rows
        if not row.get("over_budget") and row.get("stability") != "stable"
    )
    ok_count = (len(latency_rows) + len(capacity_rows)) - critical_count - watch_count
    triage_summary = {
        "critical": critical_count,
        "watch": watch_count,
        "ok": max(0, ok_count),
    }

    payload = {
        "meta": {
            "generated_from": "scripts/speed_efficiency_report.py",
            "source_artifacts": [
                str(SLOWSPOTS_PATH),
                str(SEARCH_SPEED_PATH),
                str(FLAMEGRAPHS_PATH),
                str(CAPACITY_PATH),
                str(RESOURCE_PROFILES_PATH),
                str(FRAME_METRICS_PATH),
            ],
        },
        "summary": {
            "search_scenarios": len(search_rows),
            "search_dispatch_scenarios": len(search_dispatch_rows),
            "editor_scenarios": len(editor_rows),
            "tabs_and_splits_scenarios": len(tabs_rows),
            "capacity_scenarios": len(capacity_rows),
            "resource_profile_scenarios": len(resource_profiles.get("scenarios", [])),
            "over_budget_latency": sum(
                1
                for row in search_dispatch_rows + search_rows + editor_rows + tabs_rows
                if row.get("over_budget")
            ),
            "coverage_gaps": sum(
                1
                for row in search_dispatch_rows + search_rows + editor_rows + tabs_rows
                if row.get("over_budget") and not row.get("has_profile_coverage")
            ),
            "near_failure_ceilings": sum(
                1 for row in capacity_rows if row.get("ceiling_reached")
            ),
        },
        "triage_summary": triage_summary,
        "triage": triage,
        "sections": {
            "search_dispatch": search_dispatch_rows,
            "search": search_rows,
            "editor_file_size": editor_rows,
            "tabs_and_splits": tabs_rows,
            "capacity": capacity_rows,
            "resource_profiles": resource_profiles.get("scenarios", []),
            "flamegraph_coverage": flamegraph_rows,
            "methodology": methodology_notes(),
        },
    }

    emit_report(
        payload,
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=render_cli,
        label="speed-efficiency report",
    )


if __name__ == "__main__":
    main()
