import argparse
import json
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional

from perf_report_shared import flamegraph_configs
from report_modes import add_mode_argument, emit_report

DEFAULT_OUTPUT = Path("performance_review.json")
VISIBILITY_OUTPUT = Path("target/analysis/performance_review.json")
ANALYSIS_DIR = Path("target/analysis")
MB = 1024 * 1024
GB = 1024 * MB

SOURCE_PATHS = {
    "slowspots": ANALYSIS_DIR / "slowspots.json",
    "search_speed": ANALYSIS_DIR / "search_speed.json",
    "capacity": ANALYSIS_DIR / "capacity_report.json",
    "resources": ANALYSIS_DIR / "resource_profiles.json",
    "flamegraphs": ANALYSIS_DIR / "flamegraphs.json",
    "speed_report": ANALYSIS_DIR / "speed_efficiency_report.json",
}

PROBE_CLASSES = {
    "ceiling_health": {
        "label": "Ceiling / Promise Health",
        "purpose": "Shows whether a seven-promise scale boundary still passes.",
    },
    "targeted_path": {
        "label": "Targeted Path",
        "purpose": "Shows whether a specific implementation path or recent change is working.",
    },
    "diagnostic_profile": {
        "label": "Diagnostic Profile",
        "purpose": "Explains where time goes after a targeted or ceiling probe finds pressure.",
    },
}

SCENARIOS: List[Dict[str, Any]] = [
    {
        "id": "large_files",
        "title": "Large Files",
        "promise": "Load, inspect, scroll, and edit very large text files quickly.",
        "families": ["file-load", "scroll", "viewport", "snapshot", "text-layout"],
        "benchmark_keys": [
            "file_load",
            "file_open_latency",
            "ui_render_frame_120hz",
            "ui_render_frame",
            "editor_scroll_frame_120hz",
            "scroll_stress_latency",
            "document_snapshot_creation_latency",
            "viewport_extraction_latency",
        ],
        "capacity_scenarios": ["file_size_ceiling", "text_layout_ceiling"],
        "resource_scenarios": [
            "large_utf8_load_peak_memory",
            "file_backed_open_first_visible_paint",
            "file_backed_open_allocation",
        ],
        "profile_ids": [
            "scroll_stress_profile",
            "viewport_extraction_profile",
            "document_snapshot_profile",
        ],
        "scale_checks": [
            {
                "id": "gb_file",
                "label": "GB-class file",
                "kind": "bytes",
                "minimum": GB,
                "sources": [
                    "file_size_ceiling",
                    "large_utf8_load_peak_memory",
                    "file_backed_open_first_visible_paint",
                    "file_backed_open_allocation",
                ],
            },
            {
                "id": "text_layout_batch",
                "label": "Text layout batch",
                "kind": "bytes",
                "minimum": 8 * MB,
                "sources": ["text_layout_ceiling"],
            },
            {
                "id": "visible_100ms",
                "label": "Sub-100ms visible response",
                "kind": "latency_budget_ms",
                "maximum": 100.0,
            },
        ],
        "next_measurement": "Use the GB+ file-backed open and first-visible-paint sweep for user-visible file goals; keep Text Layout as the raw egui layout boundary canary.",
    },
    {
        "id": "many_files",
        "title": "Many Files",
        "promise": "Keep workspace and file workflows responsive above 10,000 files.",
        "families": ["many-files", "file-load", "session-persistence", "search", "search-dispatch"],
        "benchmark_keys": [
            "search_current_completion_aggregate_size",
            "search_all_completion_aggregate_size",
            "search_current_dispatch_aggregate_size",
            "search_all_dispatch_aggregate_size",
        ],
        "capacity_scenarios": ["many_file_count_ceiling", "search_target_count_ceiling"],
        "resource_scenarios": [
            "many_file_resource_tracking",
            "many_file_lazy_open_tracking",
            "search_target_resource_tracking",
            "session_persist_cost",
            "session_restore_cost",
        ],
        "profile_ids": ["search_all_tabs_profile", "search_dispatch_profile"],
        "scale_checks": [
            {
                "id": "ten_k_files",
                "label": "10,000+ files",
                "kind": "files",
                "minimum": 10_000,
                "sources": [
                    "many_file_count_ceiling",
                    "many_file_resource_tracking",
                    "many_file_lazy_open_tracking",
                    "search_target_count_ceiling",
                    "search_target_resource_tracking",
                ],
            },
            {
                "id": "workspace_restore_10k",
                "label": "10,000-file restore",
                "kind": "tabs",
                "minimum": 10_000,
                "sources": ["session_persist_cost", "session_restore_cost"],
            },
        ],
        "next_measurement": "Extend lazy-open coverage to 100k-file corpora and add all-files search probes over cold tabs.",
    },
    {
        "id": "search",
        "title": "Search",
        "promise": "Return first matches quickly and finish searches over huge files and many files.",
        "families": ["search", "search-dispatch"],
        "benchmark_keys": [
            "buffer_search_regex",
            "search_active_completion_file_size",
            "search_active_first_response_file_size",
            "search_current_completion_file_size",
            "search_current_first_response_file_size",
            "search_all_completion_file_size",
            "search_all_first_response_file_size",
            "search_current_completion_aggregate_size",
            "search_current_first_response_aggregate_size",
            "search_current_app_state_completion_aggregate_size",
            "search_all_completion_aggregate_size",
            "search_all_first_response_aggregate_size",
            "search_current_dispatch_aggregate_size",
            "search_all_dispatch_aggregate_size",
            "search_capacity",
        ],
        "capacity_scenarios": ["search_file_size_ceiling", "search_target_count_ceiling"],
        "resource_scenarios": [
            "search_file_size_resource_tracking",
            "search_target_resource_tracking",
            "search_app_result_tracking",
            "edited_buffer_search_preview_rendering",
        ],
        "profile_ids": [
            "search_current_app_state_profile",
            "search_all_tabs_profile",
            "search_dispatch_profile",
            "search_capacity_profile",
        ],
        "scale_checks": [
            {
                "id": "gb_search",
                "label": "GB-class single-file search",
                "kind": "bytes",
                "minimum": GB,
                "sources": [
                    "search_file_size_ceiling",
                    "search_file_size_resource_tracking",
                    "edited_buffer_search_preview_rendering",
                ],
            },
            {
                "id": "ten_k_search_targets",
                "label": "10,000+ search targets",
                "kind": "files",
                "minimum": 10_000,
                "sources": [
                    "search_target_count_ceiling",
                    "search_target_resource_tracking",
                    "search_app_result_tracking",
                ],
            },
            {
                "id": "first_response_budget",
                "label": "First response budgeted",
                "kind": "first_response_rows",
                "minimum": 1,
            },
        ],
        "next_measurement": "Add first-response and completion sweeps for a 10k-file, GB-scale corpus.",
    },
    {
        "id": "many_tabs",
        "title": "Many Tabs",
        "promise": "Open, switch, reorder, and manipulate huge tab sets quickly.",
        "families": ["tab-management"],
        "benchmark_keys": ["tab_stress_operations", "tab_count_scale"],
        "capacity_scenarios": ["tab_count_ceiling"],
        "resource_scenarios": [
            "tab_count_resource_tracking",
            "tab_build_targeted",
            "tab_split_targeted",
            "tab_combine_targeted",
            "tab_strip_frame_rendering",
            "session_persist_cost",
            "session_restore_cost",
            "startup_visible_restore_cost",
        ],
        "profile_ids": ["tab_operations_profile", "tab_tile_layout_profile"],
        "scale_checks": [
            {
                "id": "ten_k_tabs",
                "label": "10,000+ tabs",
                "kind": "tabs",
                "minimum": 10_000,
                "sources": [
                    "tab_count_ceiling",
                    "tab_count_resource_tracking",
                    "tab_build_targeted",
                    "tab_split_targeted",
                    "tab_combine_targeted",
                    "tab_strip_frame_rendering",
                    "startup_visible_restore_cost",
                ],
            },
        ],
        "next_measurement": "Add per-frame tab-strip rendering trends for horizontal and vertical tab lists.",
    },
    {
        "id": "many_views",
        "title": "Many Views",
        "promise": "Keep many views into the same loaded files responsive.",
        "families": ["split-layout", "viewport"],
        "benchmark_keys": ["split_stress_latency", "tile_count_scale", "viewport_extraction_latency"],
        "capacity_scenarios": ["split_count_ceiling", "view_count_ceiling"],
        "resource_scenarios": ["view_count_resource_tracking", "anchor_heavy_view_editing"],
        "profile_ids": [
            "split_stress_profile",
            "tab_tile_layout_profile",
            "view_navigation_profile",
            "viewport_extraction_profile",
        ],
        "scale_checks": [
            {
                "id": "many_views",
                "label": "1,000+ views/splits",
                "kind": "views",
                "minimum": 1_000,
                "sources": [
                    "view_count_ceiling",
                    "view_count_resource_tracking",
                    "anchor_heavy_view_editing",
                    "split_count_ceiling",
                ],
            },
        ],
        "next_measurement": "Add a 1,000+ view stress profile with navigation, close, promote, and redraw steps.",
    },
    {
        "id": "text_mutation",
        "title": "Large Text Mutation",
        "promise": "Paste, cut, undo, redo, and metadata refresh should stay fast on huge buffers.",
        "families": ["edit-paste", "anchor-maintenance"],
        "benchmark_keys": ["paste_stress_latency", "piece_tree_anchor_remove"],
        "capacity_scenarios": ["paste_size_ceiling"],
        "resource_scenarios": [
            "paste_allocation",
            "provenance_retained_memory",
            "fragmented_long_session_mutation",
        ],
        "profile_ids": ["paste_stress_profile"],
        "scale_checks": [
            {
                "id": "hundred_mb_mutation",
                "label": "100 MB+ mutation",
                "kind": "bytes",
                "minimum": 100 * MB,
                "sources": [
                    "paste_size_ceiling",
                    "paste_allocation",
                    "fragmented_long_session_mutation",
                ],
            },
        ],
        "next_measurement": "Split paste, cut, undo, redo, and metadata refresh into separate large-buffer probes.",
    },
    {
        "id": "session_restore",
        "title": "Session Persistence Restore",
        "promise": "Persist and restore very large workspaces without startup stalls.",
        "families": ["session-persistence"],
        "benchmark_keys": [],
        "capacity_scenarios": [],
        "resource_scenarios": ["session_persist_cost", "session_restore_cost", "startup_visible_restore_cost"],
        "profile_ids": [],
        "scale_checks": [
            {
                "id": "ten_k_session_tabs",
                "label": "10,000+ restored tabs",
                "kind": "tabs",
                "minimum": 10_000,
                "sources": ["session_persist_cost", "session_restore_cost", "startup_visible_restore_cost"],
            },
        ],
    },
]


def load_json(path: Path, default: Any) -> Any:
    if not path.exists():
        return default
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default


def normalize_capacity_row(row: Dict[str, Any]) -> Dict[str, Any]:
    if row.get("scenario") != "layout_bytes_ceiling":
        return row
    normalized = dict(row)
    normalized["scenario"] = "text_layout_ceiling"
    normalized["scenario_label"] = "Text Layout"
    normalized["workload_family"] = "text-layout"
    return normalized


def source_status() -> List[Dict[str, Any]]:
    statuses = []
    for key, path in SOURCE_PATHS.items():
        payload = load_json(path, None)
        meta = payload.get("meta", {}) if isinstance(payload, dict) else {}
        summary = payload.get("summary", {}) if isinstance(payload, dict) else {}
        statuses.append(
            {
                "id": key,
                "path": str(path),
                "available": path.exists(),
                "status": meta.get("probe_status") or summary.get("probe_status") or "loaded" if path.exists() else "missing",
                "error": meta.get("error") or summary.get("error"),
                "record_count": summary.get("scenario_count") if isinstance(summary, dict) else None,
            }
        )
    return statuses


def row_key(row: Dict[str, Any]) -> str:
    return str(row.get("benchmark_key") or row.get("scenario") or row.get("name") or row.get("id") or "")


def row_family(row: Dict[str, Any]) -> str:
    return str(row.get("workload_family") or row.get("family") or "unmapped")


def unique_rows(rows: Iterable[Dict[str, Any]]) -> List[Dict[str, Any]]:
    seen = set()
    unique = []
    for row in rows:
        key = (
            row_key(row),
            row.get("name"),
            row.get("scenario"),
            row.get("parameter_value"),
            row.get("scenario_label"),
        )
        if key in seen:
            continue
        seen.add(key)
        unique.append(row)
    return unique


def matches_scenario(row: Dict[str, Any], scenario: Dict[str, Any]) -> bool:
    key = row_key(row)
    family = row_family(row)
    return key in scenario.get("benchmark_keys", []) or family in scenario.get("families", [])


def scenario_rows(rows: Iterable[Dict[str, Any]], scenario: Dict[str, Any]) -> List[Dict[str, Any]]:
    return unique_rows(row for row in rows if matches_scenario(row, scenario))


def scenario_named_rows(
    rows: Iterable[Dict[str, Any]],
    scenario: Dict[str, Any],
    scenario_field: str,
    accepted: str,
) -> List[Dict[str, Any]]:
    accepted_names = set(scenario.get(accepted, []))
    families = set(scenario.get("families", []))
    return unique_rows(
        row
        for row in rows
        if str(row.get(scenario_field)) in accepted_names or row_family(row) in families
    )


def fallback_flamegraphs(flamegraphs: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    if flamegraphs:
        return flamegraphs
    return [
        {
            "id": config["id"],
            "name": config["name"],
            "available": False,
            "benchmark_keys": list(config.get("benchmark_keys", [])),
            "workload_families": list(config.get("workload_families", [])),
            "coverage_role": config.get("coverage_role", "report-driven"),
            "resource_focus": config.get("resource_focus", "cpu"),
            "description": config.get("description", ""),
        }
        for config in flamegraph_configs()
    ]


def scenario_profiles(
    flamegraphs: Iterable[Dict[str, Any]],
    scenario: Dict[str, Any],
) -> List[Dict[str, Any]]:
    profile_ids = set(scenario.get("profile_ids", []))
    families = set(scenario.get("families", []))
    benchmark_keys = set(scenario.get("benchmark_keys", []))
    return unique_rows(
        item
        for item in flamegraphs
        if str(item.get("id")) in profile_ids
        or any(family in families for family in item.get("workload_families", []))
        or any(key in benchmark_keys for key in item.get("benchmark_keys", []))
    )


def mean_ms(row: Dict[str, Any]) -> Optional[float]:
    if row.get("mean_ms") is not None:
        return float(row.get("mean_ms") or 0.0)
    if row.get("mean_ns") is not None:
        return float(row.get("mean_ns") or 0.0) / 1_000_000.0
    return None


def threshold_ms(row: Dict[str, Any]) -> Optional[float]:
    if row.get("budget_ms") is not None:
        return float(row.get("budget_ms") or 0.0)
    if row.get("threshold_ms") is not None:
        return float(row.get("threshold_ms") or 0.0)
    return None


def over_budget(row: Dict[str, Any]) -> bool:
    mean = mean_ms(row)
    threshold = threshold_ms(row)
    return mean is not None and threshold is not None and threshold > 0 and mean > threshold


def max_latency_ms(rows: Iterable[Dict[str, Any]]) -> Optional[float]:
    values = [value for row in rows if (value := mean_ms(row)) is not None]
    return max(values) if values else None


def numeric_field_values(row: Dict[str, Any], fields: Iterable[str]) -> List[float]:
    return [
        float(value)
        for field in fields
        if isinstance((value := row.get(field)), (int, float)) and value > 0
    ]


def numeric_parameter_value(row: Dict[str, Any], allowed_units: set[str]) -> List[float]:
    value = row.get("parameter_value")
    unit = str(row.get("parameter_unit") or "")
    if unit in allowed_units and isinstance(value, (int, float)):
        return [float(value)]
    return []


def sample_matches_kind(kind: str, label: str) -> bool:
    label = label.lower()
    return (
        (kind == "bytes" and any(token in label for token in ("kb", "mb", "gb", "bytes")))
        or (kind == "files" and "file" in label)
        or (kind == "tabs" and "tab" in label)
        or (kind == "views" and ("split" in label or "view" in label))
    )


def labeled_sample_values(row: Dict[str, Any], kind: str) -> List[float]:
    values = []
    for sample in row.get("samples", []) or []:
        value = sample.get("workload_value")
        label = str(sample.get("workload_label") or "")
        if isinstance(value, (int, float)) and sample_matches_kind(kind, label):
            values.append(float(value))
    return values


def sample_values(rows: Iterable[Dict[str, Any]], kind: str, sources: Optional[set[str]] = None) -> List[float]:
    values: List[float] = []
    for row in rows:
        row_name = str(row.get("scenario") or row.get("benchmark_key") or row.get("name") or "")
        if sources and row_name not in sources:
            continue

        if kind == "bytes":
            values.extend(numeric_field_values(row, ("total_bytes", "bytes_per_item")))
            values.extend(numeric_parameter_value(row, {"bytes"}))
        elif kind == "files":
            values.extend(numeric_field_values(row, ("item_count", "fixed_item_count")))
            values.extend(numeric_parameter_value(row, {"files", "items", "tabs"}))
        elif kind in {"tabs", "views"}:
            values.extend(numeric_parameter_value(row, {kind, "tabs", "splits", "views"}))

        values.extend(labeled_sample_values(row, kind))
    return values


def evaluate_scale_check(
    check: Dict[str, Any],
    evidence_rows: List[Dict[str, Any]],
    latency_rows: List[Dict[str, Any]],
) -> Dict[str, Any]:
    kind = str(check.get("kind"))
    sources = set(check.get("sources", [])) or None
    if kind == "latency_budget_ms":
        budget = float(check.get("maximum", 0.0))
        matching = [row for row in latency_rows if (mean := mean_ms(row)) is not None and mean <= budget]
        fastest = min((mean_ms(row) for row in latency_rows if mean_ms(row) is not None), default=None)
        return {
            "id": check["id"],
            "label": check["label"],
            "kind": kind,
            "target": budget,
            "observed": fastest,
            "met": bool(matching),
            "unit": "ms",
        }
    if kind == "first_response_rows":
        rows = [
            row
            for row in latency_rows
            if str(row.get("latency_kind") or "").lower() == "first_response"
            or "first_response" in row_key(row)
        ]
        return {
            "id": check["id"],
            "label": check["label"],
            "kind": kind,
            "target": int(check.get("minimum", 1)),
            "observed": len(rows),
            "met": len(rows) >= int(check.get("minimum", 1)),
            "unit": "rows",
        }

    values = sample_values(evidence_rows, kind, sources)
    observed = max(values) if values else None
    minimum = float(check.get("minimum", 0.0))
    return {
        "id": check["id"],
        "label": check["label"],
        "kind": kind,
        "target": minimum,
        "observed": observed,
        "met": observed is not None and observed >= minimum,
        "unit": {
            "bytes": "bytes",
            "files": "files",
            "tabs": "tabs",
            "views": "views",
        }.get(kind, "value"),
    }


def format_target(value: Any, unit: str) -> str:
    if value is None:
        return "-"
    if unit == "bytes":
        if value >= GB:
            return f"{value / GB:.1f} GB"
        if value >= MB:
            return f"{value / MB:.1f} MB"
    if unit == "ms":
        return f"{float(value):.1f} ms"
    if isinstance(value, float) and value.is_integer():
        return f"{int(value):,}"
    if isinstance(value, (int, float)):
        return f"{value:,}"
    return str(value)


def coverage_axis(
    rows: List[Dict[str, Any]],
    *,
    label: str,
    required: bool = True,
) -> Dict[str, Any]:
    return {
        "label": label,
        "count": len(rows),
        "covered": bool(rows),
        "required": required,
    }


def scenario_gaps(
    scenario: Dict[str, Any],
    axes: Dict[str, Dict[str, Any]],
    scale_checks: List[Dict[str, Any]],
    latency_rows: List[Dict[str, Any]],
    capacity_rows: List[Dict[str, Any]],
    profile_rows: List[Dict[str, Any]],
) -> List[str]:
    gaps: List[str] = []
    for key, axis in axes.items():
        if axis.get("required") and not axis.get("covered"):
            gaps.append(f"Missing {key} evidence.")
    for check in scale_checks:
        if not check.get("met"):
            observed = format_target(check.get("observed"), check.get("unit", "value"))
            target = format_target(check.get("target"), check.get("unit", "value"))
            gaps.append(f"{check['label']} target not proven: observed {observed}, target {target}.")
    if latency_rows and not any(row.get("median_ns") for row in latency_rows):
        gaps.append("Latency rows do not expose true p95 or tail-latency coverage.")
    if capacity_rows and not any(row.get("resource_checklist") for row in capacity_rows):
        gaps.append("Capacity rows lack per-resource USE checklist detail.")
    if profile_rows and not any(item.get("available") for item in profile_rows):
        gaps.append("Profile coverage is indexed, but no SVG is currently available.")
    if scenario["id"] == "many_files" and axes["capacity"]["count"] == 0:
        gaps.append("No dedicated many-file capacity sweep exists.")
    return gaps


def scenario_opportunities(
    scenario: Dict[str, Any],
    gaps: List[str],
    latency_rows: List[Dict[str, Any]],
    capacity_rows: List[Dict[str, Any]],
    resource_rows: List[Dict[str, Any]],
) -> List[str]:
    opportunities = []
    if scenario.get("next_measurement"):
        opportunities.append(scenario["next_measurement"])
    if any(over_budget(row) for row in latency_rows):
        opportunities.append("Promote over-budget rows into focused profile runs before changing app code.")
    if any(row.get("ceiling_reached") for row in capacity_rows):
        opportunities.append("Use the first failed ceiling as the next repro scale instead of starting from tiny cases.")
    if resource_rows and not capacity_rows:
        opportunities.append("Add a matching capacity ceiling so resource growth has a pass/fail boundary.")
    if any("p95" in gap or "tail-latency" in gap for gap in gaps):
        opportunities.append("Store tail latency explicitly so review can distinguish spikes from steady regressions.")
    return unique_strings(opportunities)


def unique_strings(values: Iterable[str]) -> List[str]:
    seen = set()
    unique = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        unique.append(value)
    return unique


def compact_latency_row(row: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "id": row.get("name") or row.get("scenario_id") or row_key(row),
        "probe_class": "targeted_path",
        "measurement_role": "change_validation",
        "measurement_question": "Did this implementation path stay inside its latency budget?",
        "benchmark_key": row_key(row),
        "label": row.get("scenario_label") or row.get("name") or row_key(row),
        "family": row_family(row),
        "mean_ms": mean_ms(row),
        "budget_ms": threshold_ms(row),
        "over_budget": over_budget(row),
        "stability": row.get("stability", "stable"),
        "signals": row.get("signals", ""),
        "matching_flamegraphs": row.get("matching_flamegraphs", []),
        "parameter_label": row.get("parameter_label"),
        "item_count": row.get("item_count"),
        "total_bytes": row.get("total_bytes"),
    }


def compact_capacity_row(row: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "id": row.get("scenario"),
        "probe_class": "ceiling_health",
        "measurement_role": "promise_health",
        "measurement_question": "Does this promise still pass as workload size increases?",
        "label": row.get("scenario_label") or row.get("scenario"),
        "failure_mode": row.get("failure_mode", "not_reached"),
        "ceiling_reached": bool(row.get("ceiling_reached")),
        "last_successful_label": row.get("last_successful_label"),
        "first_failure_label": row.get("first_failure_label"),
        "suspected_limiting_resource": row.get("suspected_limiting_resource"),
        "peak_working_set_bytes": row.get("peak_working_set_bytes"),
        "working_set_growth_bytes": row.get("working_set_growth_bytes"),
        "matching_flamegraphs": row.get("matching_flamegraphs", []),
    }


def compact_resource_row(row: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "id": row.get("scenario"),
        "probe_class": "targeted_path",
        "measurement_role": "change_validation",
        "measurement_question": "Did this targeted path keep resource growth bounded?",
        "label": row.get("scenario_label") or row.get("scenario"),
        "family": row_family(row),
        "focus": row.get("focus"),
        "sample_count": row.get("sample_count"),
        "max_elapsed_ms": row.get("max_elapsed_ms"),
        "max_allocated_bytes": row.get("max_allocated_bytes"),
        "max_peak_live_bytes": row.get("max_peak_live_bytes"),
        "max_working_set_bytes": row.get("max_working_set_bytes"),
        "page_fault_growth": row.get("page_fault_growth"),
        "handle_growth": row.get("handle_growth"),
        "max_manifest_size_bytes": row.get("max_manifest_size_bytes"),
    }


def compact_profile_row(row: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "id": row.get("id"),
        "probe_class": "diagnostic_profile",
        "measurement_role": "diagnosis",
        "measurement_question": "Which hot path should explain a failing or noisy measurement?",
        "name": row.get("name") or row.get("id"),
        "available": bool(row.get("available")),
        "families": row.get("workload_families", []),
        "benchmark_keys": row.get("benchmark_keys", []),
        "issue": row.get("issue"),
    }


def best_workload_label(row: Dict[str, Any]) -> str:
    if row.get("first_failure_label"):
        return str(row.get("first_failure_label"))
    if row.get("last_successful_label"):
        return str(row.get("last_successful_label"))
    samples = row.get("samples", []) or []
    if samples:
        return str(samples[-1].get("workload_label") or samples[-1].get("workload_value") or "-")
    return str(row.get("parameter_label") or row.get("scenario_label") or row.get("name") or "-")


def build_implementation_rows(
    latency_rows: List[Dict[str, Any]],
    capacity_rows: List[Dict[str, Any]],
    resource_rows: List[Dict[str, Any]],
    profile_rows: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    implementations: List[Dict[str, Any]] = []

    for row in capacity_rows:
        implementations.append(
            {
                "kind": "ceiling",
                "probe_class": "ceiling_health",
                "label": row.get("scenario_label") or row.get("scenario"),
                "measurement": best_workload_label(row),
                "status": row.get("failure_mode", "measured"),
                "detail": f"promise health; resource={row.get('suspected_limiting_resource', '-')}",
            }
        )

    for row in resource_rows:
        implementations.append(
            {
                "kind": "resource",
                "probe_class": "targeted_path",
                "label": row.get("scenario_label") or row.get("scenario"),
                "measurement": best_workload_label(row),
                "status": row.get("focus", "resource"),
                "detail": f"targeted path; peak={format_target(row.get('max_peak_live_bytes'), 'bytes')} elapsed={format_target(row.get('max_elapsed_ms'), 'ms')}",
            }
        )

    top_latency = sorted(
        latency_rows,
        key=lambda item: mean_ms(item) or 0.0,
        reverse=True,
    )[:3]
    for row in top_latency:
        implementations.append(
            {
                "kind": "speed",
                "probe_class": "targeted_path",
                "label": row.get("scenario_label") or row.get("name") or row_key(row),
                "measurement": format_target(mean_ms(row), "ms"),
                "status": "over budget" if over_budget(row) else "measured",
                "detail": f"targeted path; budget={format_target(threshold_ms(row), 'ms')}",
            }
        )

    available_profiles = [row for row in profile_rows if row.get("available")]
    if profile_rows:
        implementations.append(
            {
                "kind": "profile",
                "probe_class": "diagnostic_profile",
                "label": "Profile coverage",
                "measurement": f"{len(available_profiles)}/{len(profile_rows)} SVGs",
                "status": "available" if available_profiles else "indexed",
                "detail": ", ".join(str(row.get("id")) for row in profile_rows[:3]),
            }
        )

    return implementations[:8]


def build_scenario(
    scenario: Dict[str, Any],
    latency_rows: List[Dict[str, Any]],
    capacity_rows: List[Dict[str, Any]],
    resource_rows: List[Dict[str, Any]],
    flamegraphs: List[Dict[str, Any]],
) -> Dict[str, Any]:
    scenario_latency = scenario_rows(latency_rows, scenario)
    scenario_capacity = scenario_named_rows(capacity_rows, scenario, "scenario", "capacity_scenarios")
    scenario_resources = scenario_named_rows(resource_rows, scenario, "scenario", "resource_scenarios")
    scenario_profile_rows = scenario_profiles(flamegraphs, scenario)
    axes = {
        "speed": coverage_axis(
            scenario_latency,
            label="Speed",
            required=bool(scenario.get("benchmark_keys")),
        ),
        "capacity": coverage_axis(
            scenario_capacity,
            label="Capacity",
            required=bool(scenario.get("capacity_scenarios")),
        ),
        "resource": coverage_axis(
            scenario_resources,
            label="Resources",
            required=bool(scenario.get("resource_scenarios")),
        ),
        "profiles": coverage_axis(
            scenario_profile_rows,
            label="Profiles",
            required=bool(scenario.get("profile_ids")),
        ),
    }
    evidence_rows = scenario_latency + scenario_capacity + scenario_resources
    scale_checks = [
        evaluate_scale_check(check, evidence_rows, scenario_latency)
        for check in scenario.get("scale_checks", [])
    ]
    gaps = scenario_gaps(
        scenario,
        axes,
        scale_checks,
        scenario_latency,
        scenario_capacity,
        scenario_profile_rows,
    )
    required_axes = [axis for axis in axes.values() if axis.get("required")]
    covered_required = sum(1 for axis in required_axes if axis.get("covered"))
    scale_met = sum(1 for check in scale_checks if check.get("met"))
    scale_total = len(scale_checks)
    required_total = len(required_axes)
    coverage_score = (covered_required + scale_met) / max(1, required_total + scale_total)
    if covered_required == required_total and scale_met == scale_total:
        status = "covered"
    elif covered_required > 0 or scale_met > 0:
        status = "thin"
    else:
        status = "missing"
    implementations = build_implementation_rows(
        scenario_latency,
        scenario_capacity,
        scenario_resources,
        scenario_profile_rows,
    )
    ceiling_probe_count = len(scenario_capacity)
    targeted_probe_count = len(scenario_latency) + len(scenario_resources)
    diagnostic_profile_count = len(scenario_profile_rows)

    return {
        "id": scenario["id"],
        "title": scenario["title"],
        "promise": scenario["promise"],
        "coverage_status": status,
        "coverage_score": round(coverage_score, 3),
        "axes": axes,
        "scale_checks": scale_checks,
        "gaps": gaps,
        "opportunities": scenario_opportunities(
            scenario,
            gaps,
            scenario_latency,
            scenario_capacity,
            scenario_resources,
        ),
        "implementations": implementations,
        "implementation_count": len(implementations),
        "measurement_split": {
            "ceiling_health": {
                **PROBE_CLASSES["ceiling_health"],
                "count": ceiling_probe_count,
                "ceilings_reached": sum(1 for row in scenario_capacity if row.get("ceiling_reached")),
            },
            "targeted_path": {
                **PROBE_CLASSES["targeted_path"],
                "count": targeted_probe_count,
                "budget_misses": sum(1 for row in scenario_latency if over_budget(row)),
            },
            "diagnostic_profile": {
                **PROBE_CLASSES["diagnostic_profile"],
                "count": diagnostic_profile_count,
                "available": sum(1 for row in scenario_profile_rows if row.get("available")),
            },
        },
        "budget_misses": sum(1 for row in scenario_latency if over_budget(row)),
        "ceilings_reached": sum(1 for row in scenario_capacity if row.get("ceiling_reached")),
        "max_latency_ms": max_latency_ms(scenario_latency),
        "peak_working_set_bytes": max(
            [
                int(value)
                for value in [
                    *(row.get("peak_working_set_bytes") for row in scenario_capacity),
                    *(row.get("max_working_set_bytes") for row in scenario_resources),
                ]
                if isinstance(value, (int, float))
            ],
            default=None,
        ),
        "evidence": {
            "latency": [compact_latency_row(row) for row in scenario_latency],
            "capacity": [compact_capacity_row(row) for row in scenario_capacity],
            "resources": [compact_resource_row(row) for row in scenario_resources],
            "profiles": [compact_profile_row(row) for row in scenario_profile_rows],
        },
    }


def build_opportunities(scenarios: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    rows = []
    for scenario in scenarios:
        severity = 3 if scenario["coverage_status"] == "missing" else 2 if scenario["coverage_status"] == "thin" else 1
        severity += min(3, len(scenario.get("gaps", []))) / 10.0
        severity += min(2, scenario.get("budget_misses", 0)) / 10.0
        rows.append(
            {
                "scenario_id": scenario["id"],
                "scenario_title": scenario["title"],
                "reason": scenario["gaps"][0] if scenario.get("gaps") else "Coverage is present; keep it fresh.",
                "recommended_action": (scenario.get("opportunities") or ["Keep coverage fresh."])[0],
                "rank_score": round(severity, 2),
            }
        )
    return sorted(rows, key=lambda item: item["rank_score"], reverse=True)


def presentation_notes() -> List[str]:
    return [
        "Lead with scenario coverage before raw dataset tables.",
        "Keep ceiling probes and targeted path probes visually separate: ceilings prove promise health; targeted paths validate changes.",
        "Show target scale status for GB files, 10,000+ files, 10,000+ tabs, and 1,000+ views.",
        "Render gaps inline with each scenario card.",
        "Keep latency, capacity, resources, and flamegraphs available as drill-down evidence.",
        "Track missing scale targets as a dashboard history metric.",
    ]


def build_payload() -> Dict[str, Any]:
    slowspots = load_json(SOURCE_PATHS["slowspots"], [])
    search_speed = load_json(SOURCE_PATHS["search_speed"], [])
    capacity_report = load_json(SOURCE_PATHS["capacity"], {"scenarios": []})
    resource_profiles = load_json(SOURCE_PATHS["resources"], {"scenarios": []})
    flamegraphs = fallback_flamegraphs(load_json(SOURCE_PATHS["flamegraphs"], []))
    speed_report = load_json(SOURCE_PATHS["speed_report"], {})

    latency_rows = unique_rows([*slowspots, *search_speed])
    capacity_rows = (
        [
            normalize_capacity_row(row)
            for row in capacity_report.get("scenarios", [])
            if isinstance(row, dict)
        ]
        if isinstance(capacity_report, dict)
        else []
    )
    resource_rows = resource_profiles.get("scenarios", []) if isinstance(resource_profiles, dict) else []
    scenario_rows_payload = [
        build_scenario(scenario, latency_rows, capacity_rows, resource_rows, flamegraphs)
        for scenario in SCENARIOS
    ]
    missing_scale_targets = sum(
        1
        for scenario in scenario_rows_payload
        for check in scenario.get("scale_checks", [])
        if not check.get("met")
    )
    coverage_gaps = sum(len(scenario.get("gaps", [])) for scenario in scenario_rows_payload)
    covered = sum(1 for scenario in scenario_rows_payload if scenario["coverage_status"] == "covered")
    thin = sum(1 for scenario in scenario_rows_payload if scenario["coverage_status"] == "thin")
    missing = sum(1 for scenario in scenario_rows_payload if scenario["coverage_status"] == "missing")
    budget_misses = sum(scenario.get("budget_misses", 0) for scenario in scenario_rows_payload)
    ceilings_reached = sum(scenario.get("ceilings_reached", 0) for scenario in scenario_rows_payload)
    implementation_count = sum(scenario.get("implementation_count", 0) for scenario in scenario_rows_payload)
    scenario_ceiling_health_evidence = sum(
        scenario.get("measurement_split", {}).get("ceiling_health", {}).get("count", 0)
        for scenario in scenario_rows_payload
    )
    scenario_targeted_path_evidence = sum(
        scenario.get("measurement_split", {}).get("targeted_path", {}).get("count", 0)
        for scenario in scenario_rows_payload
    )

    sources = source_status()
    return {
        "meta": {
            "generated_from": "scripts/performance_review.py",
            "source_artifacts": sources,
            "scenario_model": "scenario-first performance coverage",
            "probe_classes": PROBE_CLASSES,
        },
        "summary": {
            "scenario_count": len(scenario_rows_payload),
            "covered_scenarios": covered,
            "thin_scenarios": thin,
            "missing_scenarios": missing,
            "coverage_gaps": coverage_gaps,
            "missing_scale_targets": missing_scale_targets,
            "budget_misses": budget_misses,
            "ceilings_reached": ceilings_reached,
            "implementation_count": implementation_count,
            "ceiling_health_probes": len(capacity_rows),
            "targeted_path_probes": len(latency_rows) + len(resource_rows),
            "scenario_ceiling_health_evidence": scenario_ceiling_health_evidence,
            "scenario_targeted_path_evidence": scenario_targeted_path_evidence,
            "latency_rows": len(latency_rows),
            "capacity_rows": len(capacity_rows),
            "resource_rows": len(resource_rows),
            "flamegraph_rows": len(flamegraphs),
            "failed_source_artifacts": sum(
                1
                for source in sources
                if source.get("status") not in {"loaded", "completed", "fallback_completed", None}
            ),
        },
        "target_promises": [
            "GB-class text files",
            "10,000+ files",
            "10,000+ tabs",
            "1,000+ views/splits",
            "sub-100ms visible response",
            "fast first search response and full completion",
        ],
        "scenarios": scenario_rows_payload,
        "opportunities": build_opportunities(scenario_rows_payload),
        "presentation": {
            "notes": presentation_notes(),
            "primary_view": "coverage_matrix",
            "secondary_view": "scenario_drilldown",
        },
        "coordinated_triage": speed_report.get("triage", [])[:5] if isinstance(speed_report, dict) else [],
    }


def render_cli(payload: object) -> str:
    data = payload if isinstance(payload, dict) else {}
    lines = ["Performance Review Coverage"]
    for scenario in data.get("scenarios", []):
        lines.append(
            f"- {scenario['title']}: {scenario['coverage_status']} | score={scenario['coverage_score']:.2f} | gaps={len(scenario.get('gaps', []))}"
        )
    if not data.get("scenarios"):
        lines.append("No performance review scenarios generated.")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Emit scenario-first performance review coverage from existing measurement artifacts"
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help=f"Optional output JSON path. Example: {DEFAULT_OUTPUT}",
    )
    add_mode_argument(parser)
    args = parser.parse_args()
    emit_report(
        build_payload(),
        mode=args.mode,
        output_path=args.output,
        visibility_path=VISIBILITY_OUTPUT,
        cli_renderer=render_cli,
        label="performance review",
    )


if __name__ == "__main__":
    main()
