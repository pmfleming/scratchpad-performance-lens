use super::common::{array_value, budget_probe_ms, read_analysis, scenarios_array};
use super::render::render_speed_report;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
pub fn speed_efficiency_report(config: &LensConfig) -> Result<()> {
    let slowspots = read_analysis(config, "slowspots.json", json!([]));
    let search = read_analysis(config, "search_speed.json", json!([]));
    let flamegraphs = read_analysis(config, "flamegraphs.json", json!([]));
    let capacity = read_analysis(config, "capacity_report.json", json!({"scenarios": []}));
    let resources = read_analysis(config, "resource_profiles.json", json!({"scenarios": []}));
    let frame_metrics = read_analysis(config, "frame_metrics.json", json!({"scenarios": []}));

    let mut capacity_lookup = HashMap::new();
    for row in scenarios_array(&capacity) {
        if let Some(id) = row.get("scenario").and_then(Value::as_str) {
            capacity_lookup.insert(id.to_string(), row.clone());
        }
    }

    let mut search_dispatch = Vec::new();
    let mut search_rows = Vec::new();
    for row in array_value(&search) {
        let normalized = normalize_latency_row(row, &capacity_lookup);
        if normalized.get("family").and_then(Value::as_str) == Some("search-dispatch") {
            search_dispatch.push(normalized);
        } else {
            search_rows.push(normalized);
        }
    }
    let mut editor_rows = Vec::new();
    let editor_families = [
        "file-load",
        "scroll",
        "viewport",
        "snapshot",
        "edit-paste",
        "anchor-maintenance",
        "session-persistence",
        "control-char-encoding",
    ];
    for row in array_value(&slowspots) {
        if row
            .get("workload_family")
            .and_then(Value::as_str)
            .is_some_and(|family| editor_families.contains(&family))
        {
            editor_rows.push(normalize_latency_row(row, &capacity_lookup));
        }
    }
    for row in scenarios_array(&frame_metrics) {
        editor_rows.push(normalize_frame_row(row));
    }
    let mut tabs_rows = Vec::new();
    for row in array_value(&slowspots) {
        if row
            .get("workload_family")
            .and_then(Value::as_str)
            .is_some_and(|family| ["tab-management", "split-layout"].contains(&family))
        {
            tabs_rows.push(normalize_latency_row(row, &capacity_lookup));
        }
    }
    let capacity_rows: Vec<_> = scenarios_array(&capacity)
        .iter()
        .map(normalize_capacity_row)
        .collect();
    let latency_rows = [
        search_dispatch.clone(),
        search_rows.clone(),
        editor_rows.clone(),
        tabs_rows.clone(),
    ]
    .concat();
    let flamegraph_rows = flamegraph_coverage_rows(array_value(&flamegraphs), &latency_rows);
    let triage = build_triage(&latency_rows, &capacity_rows);
    let resource_rows = scenarios_array(&resources).to_vec();
    let payload = json!({
        "meta": {
            "generated_from": "rust:speed_efficiency_report",
            "source_artifacts": [
                config.output_dir.join("slowspots.json").to_string_lossy(),
                config.output_dir.join("search_speed.json").to_string_lossy(),
                config.output_dir.join("flamegraphs.json").to_string_lossy(),
                config.output_dir.join("capacity_report.json").to_string_lossy(),
                config.output_dir.join("resource_profiles.json").to_string_lossy(),
                config.output_dir.join("frame_metrics.json").to_string_lossy(),
            ],
        },
        "summary": {
            "search_scenarios": search_rows.len(),
            "search_dispatch_scenarios": search_dispatch.len(),
            "editor_scenarios": editor_rows.len(),
            "tabs_and_splits_scenarios": tabs_rows.len(),
            "capacity_scenarios": capacity_rows.len(),
            "resource_profile_scenarios": resource_rows.len(),
            "over_budget_latency": latency_rows.iter().filter(|row| row.get("over_budget").and_then(Value::as_bool).unwrap_or(false)).count(),
            "coverage_gaps": latency_rows.iter().filter(|row| row.get("over_budget").and_then(Value::as_bool).unwrap_or(false) && !row.get("has_profile_coverage").and_then(Value::as_bool).unwrap_or(false)).count(),
            "near_failure_ceilings": capacity_rows.iter().filter(|row| row.get("ceiling_reached").and_then(Value::as_bool).unwrap_or(false)).count(),
        },
        "triage_summary": triage_summary(&latency_rows, &capacity_rows),
        "triage": triage,
        "sections": {
            "search_dispatch": search_dispatch,
            "search": search_rows,
            "editor_file_size": editor_rows,
            "tabs_and_splits": tabs_rows,
            "capacity": capacity_rows,
            "resource_profiles": resource_rows,
            "flamegraph_coverage": flamegraph_rows,
            "methodology": methodology_notes(),
        },
    });
    shared::write_visibility(
        &config.output_dir.join("speed_efficiency_report.json"),
        &payload,
        "speed-efficiency report",
        render_speed_report(&payload),
    )
}

fn normalize_latency_row(row: Value, capacity_lookup: &HashMap<String, Value>) -> Value {
    let family = row
        .get("workload_family")
        .or_else(|| row.get("family"))
        .and_then(Value::as_str)
        .unwrap_or("unmapped");
    let ceiling = match family {
        "file-load" | "scroll" | "viewport" | "snapshot" => {
            capacity_lookup.get("file_size_ceiling")
        }
        "edit-paste" | "anchor-maintenance" => capacity_lookup.get("paste_size_ceiling"),
        "tab-management" => capacity_lookup.get("tab_count_ceiling"),
        "split-layout" => capacity_lookup.get("split_count_ceiling"),
        _ => None,
    };
    let mean_ms = row.get("mean_ns").and_then(Value::as_f64).unwrap_or(0.0) / 1_000_000.0;
    let budget_probe_ms = budget_probe_ms(&row).unwrap_or(mean_ms);
    let budget_ms = row
        .get("threshold_ms")
        .or_else(|| row.get("budget_ms"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let matching = row
        .get("matching_flamegraphs")
        .cloned()
        .unwrap_or_else(|| json!([]));
    json!({
        "scenario_id": row.get("name").or_else(|| row.get("scenario_id")).or_else(|| row.get("benchmark_key")).cloned(),
        "probe_class": row.get("probe_class").and_then(Value::as_str).unwrap_or("targeted_path"),
        "measurement_role": row.get("measurement_role").and_then(Value::as_str).unwrap_or("change_validation"),
        "scenario_label": row.get("scenario_label").or_else(|| row.get("name")).cloned(),
        "family": family,
        "mean_ms": mean_ms,
        "budget_probe_ms": budget_probe_ms,
        "budget_probe_label": row.get("budget_probe_label").and_then(Value::as_str).unwrap_or("mean"),
        "budget_ms": budget_ms,
        "stability": row.get("stability").and_then(Value::as_str).unwrap_or("stable"),
        "targets": row.get("targets").cloned().unwrap_or_else(|| json!([])),
        "matching_flamegraphs": matching,
        "has_profile_coverage": matching.as_array().is_some_and(|items| !items.is_empty()),
        "last_known_failure_ceiling": ceiling.and_then(|row| row.get("last_successful_label").or_else(|| row.get("first_failure_label"))).cloned(),
        "suspected_limiting_resource": row.get("suspected_limiting_resource").and_then(Value::as_str).unwrap_or("cpu"),
        "signals": row.get("signals").and_then(Value::as_str).unwrap_or("nominal"),
        "over_budget": budget_ms > 0.0 && budget_probe_ms > budget_ms,
    })
}

fn normalize_capacity_row(row: &Value) -> Value {
    json!({
        "scenario_id": row.get("scenario").cloned(),
        "probe_class": row.get("probe_class").and_then(Value::as_str).unwrap_or("ceiling_health"),
        "measurement_role": row.get("measurement_role").and_then(Value::as_str).unwrap_or("promise_health"),
        "scenario_label": row.get("scenario_label").or_else(|| row.get("scenario")).cloned(),
        "family": row.get("workload_family").and_then(Value::as_str).unwrap_or("capacity-stress"),
        "failure_mode": row.get("failure_mode").and_then(Value::as_str).unwrap_or("not_reached"),
        "ceiling_reached": row.get("ceiling_reached").and_then(Value::as_bool).unwrap_or(false),
        "last_successful_label": row.get("last_successful_label").cloned(),
        "first_failure_label": row.get("first_failure_label").cloned(),
        "matching_flamegraphs": row.get("matching_flamegraphs").cloned().unwrap_or_else(|| json!([])),
        "suspected_limiting_resource": row.get("suspected_limiting_resource").and_then(Value::as_str).unwrap_or("cpu"),
        "peak_working_set_bytes": row.get("peak_working_set_bytes").cloned(),
        "working_set_growth_bytes": row.get("working_set_growth_bytes").cloned(),
        "diagnosis_guidance": row.get("diagnosis_guidance").cloned().unwrap_or_else(|| json!([])),
    })
}

fn normalize_frame_row(row: &Value) -> Value {
    let budget_ms = row.get("budget_ms").and_then(Value::as_f64).unwrap_or(8.33);
    let p99_budget_ms = row
        .get("p99_budget_ms")
        .and_then(Value::as_f64)
        .unwrap_or(12.0);
    let p95_ms = row.get("p95_ms").and_then(Value::as_f64).unwrap_or(0.0);
    let p99_ms = row.get("p99_ms").and_then(Value::as_f64).unwrap_or(0.0);
    json!({
        "scenario_id": row.get("scenario_id").and_then(Value::as_str).unwrap_or("ui_render_frame_120hz"),
        "probe_class": "targeted_path",
        "measurement_role": "change_validation",
        "scenario_label": row.get("scenario_label").or_else(|| row.get("scenario_id")).cloned(),
        "family": row.get("workload_family").and_then(Value::as_str).unwrap_or("scroll"),
        "mean_ms": row.get("mean_ms").and_then(Value::as_f64).unwrap_or(0.0),
        "p50_ms": row.get("p50_ms").cloned(),
        "p95_ms": p95_ms,
        "p99_ms": p99_ms,
        "max_ms": row.get("max_ms").cloned(),
        "budget_ms": budget_ms,
        "p99_budget_ms": p99_budget_ms,
        "stability": "stable",
        "measurement_scope": row.get("measurement_scope").and_then(Value::as_str).unwrap_or("measured_frame_path_cpu"),
        "metric_role": row.get("metric_role").and_then(Value::as_str).unwrap_or("theoretical_frame_production_capacity"),
        "present_included": row.get("present_included").and_then(Value::as_bool).unwrap_or(false),
        "vsync": row.get("vsync").and_then(Value::as_bool).unwrap_or(false),
        "included_work": row.get("included_work").cloned().unwrap_or_else(|| json!([])),
        "omitted_work": row.get("omitted_work").cloned().unwrap_or_else(|| json!([])),
        "theoretical_fps_p99": row.get("theoretical_fps_p99").cloned(),
        "refresh_budget_utilization": row.get("refresh_budget_utilization").cloned(),
        "targets": ["src/app/app_state/frame.rs", "src/app/ui"],
        "matching_flamegraphs": ["ui_render_frame_profile"],
        "has_profile_coverage": true,
        "suspected_limiting_resource": "cpu",
        "signals": format!("p95 {p95_ms:.2} ms vs {budget_ms:.2} ms budget; p99 {p99_ms:.2} ms vs {p99_budget_ms:.2} ms budget"),
        "over_budget": row.get("over_budget").and_then(Value::as_bool).unwrap_or(p95_ms > budget_ms || p99_ms > p99_budget_ms),
        "phases": row.get("phases").cloned().unwrap_or_else(|| json!([])),
    })
}

fn flamegraph_coverage_rows(flamegraphs: Vec<Value>, latency_rows: &[Value]) -> Vec<Value> {
    let covered_keys: HashSet<String> = latency_rows
        .iter()
        .filter_map(|row| row.get("scenario_id").and_then(Value::as_str))
        .map(|id| shared::benchmark_key(id).to_string())
        .collect();
    flamegraphs
        .into_iter()
        .map(|item| {
            let covered: Vec<_> = item
                .get("benchmark_keys")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|key| key.as_str().is_some_and(|key| covered_keys.contains(key)))
                .collect();
            json!({
                "id": item.get("id").cloned(),
                "name": item.get("name").cloned(),
                "available": item.get("available").and_then(Value::as_bool).unwrap_or(false),
                "coverage_role": item.get("coverage_role").and_then(Value::as_str).unwrap_or("report-driven"),
                "benchmark_keys": item.get("benchmark_keys").cloned().unwrap_or_else(|| json!([])),
                "workload_families": item.get("workload_families").cloned().unwrap_or_else(|| json!([])),
                "covered_scenarios": covered,
                "issue": item.get("issue").cloned(),
            })
        })
        .collect()
}

fn build_triage(latency_rows: &[Value], capacity_rows: &[Value]) -> Vec<Value> {
    let mut rows = Vec::new();
    for row in latency_rows {
        let over_budget = row
            .get("over_budget")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let unstable = row
            .get("stability")
            .and_then(Value::as_str)
            .unwrap_or("stable")
            != "stable";
        if over_budget || unstable {
            rows.push(json!({
                "scenario_id": row.get("scenario_id").cloned(),
                "scenario_label": row.get("scenario_label").cloned(),
                "family": row.get("family").cloned(),
                "reason": row.get("signals").cloned(),
                "suspected_limiting_resource": row.get("suspected_limiting_resource").cloned(),
                "recommended_action": recommended_action(row),
                "rank_score": latency_rank(row),
            }));
        }
    }
    for row in capacity_rows {
        if row
            .get("ceiling_reached")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            rows.push(json!({
                "scenario_id": row.get("scenario_id").cloned(),
                "scenario_label": row.get("scenario_label").cloned(),
                "family": row.get("family").cloned(),
                "reason": format!("{} at {}", row.get("failure_mode").and_then(Value::as_str).unwrap_or("measured"), row.get("first_failure_label").or_else(|| row.get("last_successful_label")).and_then(Value::as_str).unwrap_or("-")),
                "suspected_limiting_resource": row.get("suspected_limiting_resource").cloned(),
                "recommended_action": recommended_action(row),
                "rank_score": capacity_rank(row),
            }));
        }
    }
    rows.sort_by(|left, right| {
        right
            .get("rank_score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .total_cmp(
                &left
                    .get("rank_score")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
            )
    });
    rows.truncate(5);
    rows
}

fn recommended_action(row: &Value) -> String {
    if let Some(first) = row
        .get("diagnosis_guidance")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_str)
    {
        return first.to_string();
    }
    if let Some(first) = row
        .get("matching_flamegraphs")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_str)
    {
        return format!("Inspect {first} against the over-budget scenario.");
    }
    if row.get("family").and_then(Value::as_str) == Some("search") {
        "Add or compare a search flamegraph before broad optimization work.".to_string()
    } else {
        "Add diagnosis coverage before prioritizing an optimization.".to_string()
    }
}

fn latency_rank(row: &Value) -> f64 {
    let budget = row.get("budget_ms").and_then(Value::as_f64).unwrap_or(0.0);
    let budget_probe = row
        .get("budget_probe_ms")
        .and_then(Value::as_f64)
        .or_else(|| row.get("mean_ms").and_then(Value::as_f64))
        .unwrap_or(0.0);
    let overrun = if budget > 0.0 {
        budget_probe / budget
    } else {
        budget_probe / 25.0
    };
    family_priority(
        row.get("family")
            .and_then(Value::as_str)
            .unwrap_or("unmapped"),
    ) * 10.0
        + overrun
}

fn capacity_rank(row: &Value) -> f64 {
    family_priority(
        row.get("family")
            .and_then(Value::as_str)
            .unwrap_or("capacity-stress"),
    ) * 10.0
        + if row
            .get("ceiling_reached")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            2.0
        } else {
            0.0
        }
}

fn family_priority(family: &str) -> f64 {
    match family {
        "search" | "search-dispatch" | "edit-paste" | "scroll" | "viewport" => 3.0,
        "snapshot"
        | "split-layout"
        | "tab-management"
        | "session-persistence"
        | "file-load"
        | "capacity-stress" => 2.0,
        "anchor-maintenance" | "control-char-encoding" => 1.0,
        _ => 0.0,
    }
}

fn triage_summary(latency_rows: &[Value], capacity_rows: &[Value]) -> Value {
    let critical = latency_rows
        .iter()
        .filter(|row| {
            row.get("over_budget")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count()
        + capacity_rows
            .iter()
            .filter(|row| {
                row.get("ceiling_reached")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
    let watch = latency_rows
        .iter()
        .filter(|row| {
            !row.get("over_budget")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && row
                    .get("stability")
                    .and_then(Value::as_str)
                    .unwrap_or("stable")
                    != "stable"
        })
        .count();
    let total = latency_rows.len() + capacity_rows.len();
    json!({"critical": critical, "watch": watch, "ok": total.saturating_sub(critical + watch)})
}

fn methodology_notes() -> Vec<&'static str> {
    vec![
        "Broad Criterion slowspots remain the wide detector for general latency regressions.",
        "The dedicated search report remains the authoritative scaling view for search latency.",
        "Flamegraphs explain CPU hot paths; they do not replace benchmark budgets or capacity ceilings.",
        "Capacity sweeps stay out of the latency leaderboard and record the first unusable ceiling separately.",
        "Ceiling probes answer promise-health questions; targeted path probes validate whether a specific change worked.",
        "Resource profiles capture allocation-heavy, working-set, and session-cost scenarios that are not visible in CPU flamegraphs alone.",
    ]
}
