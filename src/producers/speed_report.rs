use super::common::{budget_probe_ms, scenarios_array, values_array, AnalysisInputs};
use super::render::render_speed_report;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
pub fn speed_efficiency_report(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let inputs = AnalysisInputs::new(config);
    let slowspots = inputs.read("slowspots.json", json!([]));
    let search = inputs.read("search_speed.json", json!([]));
    let workflows = inputs.read("search_workflow_metrics.json", json!({"scenarios": []}));
    let flamegraphs = inputs.read("flamegraphs.json", json!([]));
    let capacity = inputs.read("capacity_report.json", json!({"scenarios": []}));
    let resources = inputs.read("resource_profiles.json", json!({"scenarios": []}));
    let frame_metrics = inputs.read("frame_metrics.json", json!({"scenarios": []}));

    let capacity_lookup = scenarios_array(&capacity)
        .iter()
        .filter_map(|row| Some((row["scenario"].as_str()?, row)))
        .collect();
    let (mut search_dispatch, mut search_rows): (Vec<_>, Vec<_>) = values_array(&search)
        .iter()
        .chain(scenarios_array(&workflows))
        .map(|row| normalize_latency_row(row, &capacity_lookup))
        .partition(|row| row["family"] == "search-dispatch");
    let (mut tabs_rows, mut editor_rows): (Vec<_>, Vec<_>) = values_array(&slowspots)
        .iter()
        .filter(|row| {
            matches!(
                row["workload_family"].as_str(),
                Some(
                    "file-load"
                        | "scroll"
                        | "viewport"
                        | "snapshot"
                        | "edit-paste"
                        | "anchor-maintenance"
                        | "session-persistence"
                        | "control-char-encoding"
                        | "tab-management"
                        | "split-layout"
                )
            )
        })
        .map(|row| normalize_latency_row(row, &capacity_lookup))
        .partition(|row| {
            matches!(
                row["family"].as_str(),
                Some("tab-management" | "split-layout")
            )
        });
    editor_rows.extend(
        scenarios_array(&frame_metrics)
            .iter()
            .map(normalize_frame_row),
    );
    let capacity_rows: Vec<_> = scenarios_array(&capacity)
        .iter()
        .map(|row| {
            normalize_capacity_row(
                row,
                capacity["meta"]["synthetic"] == true || capacity["summary"]["synthetic"] == true,
            )
        })
        .collect();
    let available_profiles: HashSet<_> = values_array(&flamegraphs)
        .iter()
        .filter(|row| row["available"] == true)
        .filter_map(|row| row["id"].as_str())
        .collect();
    for row in search_dispatch
        .iter_mut()
        .chain(&mut search_rows)
        .chain(&mut editor_rows)
        .chain(&mut tabs_rows)
    {
        row["has_profile_coverage"] =
            json!(row["matching_flamegraphs"].as_array().is_some_and(|ids| ids
                .iter()
                .any(|id| id
                    .as_str()
                    .is_some_and(|id| available_profiles.contains(id)))));
    }
    let latency_rows: Vec<&Value> = search_dispatch
        .iter()
        .chain(&search_rows)
        .chain(&editor_rows)
        .chain(&tabs_rows)
        .collect();
    let flamegraph_rows = flamegraph_coverage_rows(values_array(&flamegraphs), &latency_rows);
    let triage = build_triage(&latency_rows, &capacity_rows);
    let resource_rows = scenarios_array(&resources);
    let source_status: Vec<_> = [
        "slowspots.json",
        "search_speed.json",
        "search_workflow_metrics.json",
        "capacity_report.json",
        "resource_profiles.json",
        "frame_metrics.json",
        "flamegraphs.json",
    ]
    .into_iter()
    .map(|file| {
        json!({"file":file,"required":file != "flamegraphs.json",
                "status":crate::provenance::validity(&config.output_dir.join(file), &inputs.context)})
    })
    .collect();
    let complete_sources = source_status
        .iter()
        .all(|source| source["required"] != true || source["status"] == "valid");
    let payload = json!({
        "meta": {
            "generated_from": "rust:speed_efficiency_report",
            "source_status": source_status,
            "source_artifacts": [
                config.output_dir.join("slowspots.json").to_string_lossy(),
                config.output_dir.join("search_speed.json").to_string_lossy(),
                config.output_dir.join("search_workflow_metrics.json").to_string_lossy(),
                config.output_dir.join("flamegraphs.json").to_string_lossy(),
                config.output_dir.join("capacity_report.json").to_string_lossy(),
                config.output_dir.join("resource_profiles.json").to_string_lossy(),
                config.output_dir.join("frame_metrics.json").to_string_lossy(),
            ],
        },
        "summary": {
            "measurement_status": if complete_sources { "valid" } else { "incomplete" },
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

fn normalize_latency_row(row: &Value, capacity_lookup: &HashMap<&str, &Value>) -> Value {
    let family = if row["measurement_scope"] == "query_command_to_dispatch" {
        "search-dispatch"
    } else {
        row.get("workload_family")
            .or_else(|| row.get("family"))
            .and_then(Value::as_str)
            .unwrap_or("unmapped")
    };
    let ceiling = match family {
        "file-load" | "scroll" | "viewport" | "snapshot" => {
            capacity_lookup.get("large_file_background_index_ceiling")
        }
        "edit-paste" | "anchor-maintenance" => capacity_lookup.get("paste_size_ceiling"),
        "tab-management" => capacity_lookup.get("tab_count_ceiling"),
        "split-layout" => capacity_lookup.get("split_count_ceiling"),
        _ => None,
    };
    let mean_ms = row.get("mean_ms").and_then(Value::as_f64).or_else(|| {
        row.get("mean_ns")
            .and_then(Value::as_f64)
            .map(|ns| ns / 1_000_000.0)
    });
    let budget_probe_ms = budget_probe_ms(row).or(mean_ms);
    let budget_ms = row
        .get("threshold_ms")
        .or_else(|| row.get("budget_ms"))
        .and_then(Value::as_f64);
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
        "measurement_scope": row.get("measurement_scope").cloned(),
        "present_included": row.get("present_included").cloned(),
        "mean_ms": mean_ms,
        "budget_probe_ms": budget_probe_ms,
        "budget_probe_label": row.get("budget_probe_label").and_then(Value::as_str).unwrap_or("mean"),
        "budget_ms": budget_ms,
        "stability": row.get("stability").and_then(Value::as_str).unwrap_or("stable"),
        "targets": row.get("targets").cloned().unwrap_or_else(|| json!([])),
        "matching_flamegraphs": matching,
        "has_profile_coverage": matching.as_array().is_some_and(|items| !items.is_empty()),
        "last_known_failure_ceiling": ceiling.and_then(|row| row.get("last_successful_label").or_else(|| row.get("first_failure_label"))).cloned(),
        "suspected_limiting_resource": row.get("suspected_limiting_resource").and_then(Value::as_str).unwrap_or("unknown"),
        "signals": row.get("signals").and_then(Value::as_str).unwrap_or(match row["budget_status"].as_str() {
            Some("fail") => "observed budget violation", Some("pass") => "within observed budget",
            Some("uncertain") => "budget uncertain", _ => "budget status unknown",
        }),
        "budget_status": row.get("budget_status").cloned().unwrap_or(json!("unmeasured")),
        "mean_confidence_interval_ns": row.get("mean_confidence_interval_ns").cloned(),
        "measurement_status": row.get("measurement_status").cloned().unwrap_or(json!("unmeasured")),
        "over_budget": if row["measurement_status"] != "valid" { None } else { match row["budget_status"].as_str() {
            Some("pass") => Some(false), Some("fail") => Some(true), _ => None,
        } },
    })
}

fn normalize_capacity_row(row: &Value, synthetic: bool) -> Value {
    let known = !synthetic
        && row["ceiling_reached"].is_boolean()
        && !matches!(
            row["failure_mode"].as_str(),
            Some("unmeasured" | "incomplete")
        );
    json!({
        "scenario_id": row.get("scenario").cloned(),
        "probe_class": row.get("probe_class").and_then(Value::as_str).unwrap_or("ceiling_health"),
        "measurement_role": row.get("measurement_role").and_then(Value::as_str).unwrap_or("promise_health"),
        "scenario_label": row.get("scenario_label").or_else(|| row.get("scenario")).cloned(),
        "family": row.get("workload_family").and_then(Value::as_str).unwrap_or("capacity-stress"),
        "failure_mode": row.get("failure_mode").and_then(Value::as_str).unwrap_or("unmeasured"),
        "synthetic": synthetic,
        "measurement_status": if known { "valid" } else { "unmeasured" },
        "ceiling_reached": if known { row.get("ceiling_reached").cloned() } else { None },
        "last_successful_label": if synthetic { None } else { row.get("last_successful_label").cloned() },
        "first_failure_label": if synthetic { None } else { row.get("first_failure_label").cloned() },
        "matching_flamegraphs": row.get("matching_flamegraphs").cloned().unwrap_or_else(|| json!([])),
        "suspected_limiting_resource": row.get("suspected_limiting_resource").and_then(Value::as_str).unwrap_or("unknown"),
        "peak_working_set_bytes": row.get("peak_working_set_bytes").cloned(),
        "working_set_growth_bytes": row.get("working_set_growth_bytes").cloned(),
        "diagnosis_guidance": row.get("diagnosis_guidance").cloned().unwrap_or_else(|| json!([])),
    })
}

fn normalize_frame_row(row: &Value) -> Value {
    let valid = super::common::valid_frame(row);
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
        "stability": "unknown",
        "measurement_status": if valid { "valid" } else { "unmeasured" },
        "percentile_method": row.get("percentile_method").cloned(),
        "percentile_resolution_ms": row.get("percentile_resolution_ms").cloned(),
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
        "suspected_limiting_resource": "unknown",
        "signals": format!("p95 {p95_ms:.2} ms vs {budget_ms:.2} ms budget; p99 {p99_ms:.2} ms vs {p99_budget_ms:.2} ms budget"),
        "over_budget": valid.then_some(p95_ms > budget_ms || p99_ms > p99_budget_ms),
        "phases": row.get("phases").cloned().unwrap_or_else(|| json!([])),
    })
}

fn flamegraph_coverage_rows(flamegraphs: &[Value], latency_rows: &[&Value]) -> Vec<Value> {
    let covered_keys: HashSet<_> = latency_rows
        .iter()
        .filter_map(|row| row.get("scenario_id").and_then(Value::as_str))
        .map(shared::benchmark_key)
        .collect();
    flamegraphs
        .iter()
        .map(|item| {
            let covered: Vec<_> = values_array(&item["benchmark_keys"]).iter()
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

fn build_triage(latency_rows: &[&Value], capacity_rows: &[Value]) -> Vec<Value> {
    let mut rows = Vec::new();
    for row in latency_rows {
        if row["over_budget"].is_null() {
            continue;
        }
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
    let overrun = if let (Some(p95), Some(p99), Some(p99_budget)) = (
        row["p95_ms"].as_f64(),
        row["p99_ms"].as_f64(),
        row["p99_budget_ms"].as_f64(),
    ) {
        if budget > 0.0 && p99_budget > 0.0 {
            (p95 / budget).max(p99 / p99_budget)
        } else {
            0.0
        }
    } else if budget > 0.0 {
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

fn triage_summary(latency_rows: &[&Value], capacity_rows: &[Value]) -> Value {
    let (mut critical, mut watch, mut ok, mut unknown) = (0, 0, 0, 0);
    for row in latency_rows {
        match row["over_budget"].as_bool() {
            Some(true) => critical += 1,
            Some(false) if matches!(row["stability"].as_str(), Some("watch" | "high-variance")) => {
                watch += 1
            }
            Some(false) => ok += 1,
            None => unknown += 1,
        }
    }
    for row in capacity_rows {
        match row["ceiling_reached"].as_bool() {
            Some(true) => critical += 1,
            Some(false) => ok += 1,
            None => unknown += 1,
        }
    }
    json!({"critical":critical,"watch":watch,"ok":ok,"unknown":unknown})
}

fn methodology_notes() -> Vec<&'static str> {
    vec![
        "Criterion estimates component means and confidence intervals, not request latency tails.",
        "Search workflows measure real dispatch, delivery, headless render preparation and completion.",
        "Only available profiles count as diagnosis coverage; profiles do not prove budgets.",
        "Capacity health requires successful complete repetitions for every required operation.",
        "Unknown, invalid and synthetic evidence never counts as passing triage.",
        "Process high-water RSS and absolute heap baselines are not per-operation memory peaks.",
    ]
}

#[cfg(test)]
mod tests {
    use super::{
        build_triage, normalize_capacity_row, normalize_latency_row, speed_efficiency_report,
        triage_summary,
    };
    use crate::{cli::MeasureOptions, config::LensConfig, provenance};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    #[test]
    fn synthetic_and_unknown_capacity_are_not_reported_as_ok() {
        let row = normalize_capacity_row(
            &json!({"ceiling_reached":null,"failure_mode":"unmeasured"}),
            true,
        );
        assert!(row["ceiling_reached"].is_null());
        assert_eq!(row["synthetic"], true);
        let summary = triage_summary(&[&json!({"over_budget":null})], &[row]);
        assert_eq!(summary["ok"], 0);
        assert_eq!(summary["unknown"], 2);
        let missing = normalize_latency_row(
            &json!({"mean_ns":1,"measurement_status":"valid"}),
            &HashMap::new(),
        );
        assert!(missing["over_budget"].is_null());
    }

    #[test]
    fn fresh_sources_flow_through_sections_profiles_triage_and_rendering() -> anyhow::Result<()> {
        let row = normalize_latency_row(
            &json!({"mean_ms":167,"budget_probe_ns":167_000_000,
            "threshold_ms":120,"measurement_status":"valid","budget_status":"fail"}),
            &HashMap::new(),
        );
        let triage = build_triage(&[&row], &[]);
        assert_eq!(triage[0]["reason"], "observed budget violation");
        assert_eq!(triage[0]["suspected_limiting_resource"], "unknown");
        let root = tempfile::tempdir()?;
        let config = LensConfig {
            project_name: "fixture".into(),
            project_root: root.path().into(),
            output_dir: root.path().join("analysis"),
        };
        std::fs::create_dir(&config.output_dir)?;
        let context = provenance::context(root.path());
        let sources = [
            (
                "slowspots.json",
                json!([
                    {"name":"load/32","workload_family":"file-load","mean_ns":200_000_000,"threshold_ms":100,"measurement_status":"valid","budget_status":"fail","matching_flamegraphs":["missing"]},
                    {"name":"tabs/32","workload_family":"tab-management","measurement_status":"valid","budget_status":"pass"}
                ]),
            ),
            (
                "search_speed.json",
                json!([{"name":"dispatch/32","workload_family":"search","measurement_scope":"query_command_to_dispatch","mean_ms":280,"threshold_ms":50,"measurement_status":"valid","budget_status":"fail","matching_flamegraphs":["ui_render_frame_profile"]}]),
            ),
            (
                "search_workflow_metrics.json",
                json!({"scenarios":[{"name":"search/32","workload_family":"search","measurement_status":"valid","budget_status":"pass"}]}),
            ),
            (
                "frame_metrics.json",
                json!({"scenarios":[{"scenario_id":"scroll","frame_count":240,"measurement_scope":"event_to_tessellation","p95_ms":9,"p99_ms":15,"budget_ms":8.33,"p99_budget_ms":12}]}),
            ),
            (
                "capacity_report.json",
                json!({"scenarios":[{"scenario":"tab_count_ceiling","ceiling_reached":true,"failure_mode":"unusable_latency","last_successful_label":"32 tabs","first_failure_label":"64 tabs","workload_family":"tab-management","diagnosis_guidance":["inspect counters"]}]}),
            ),
            (
                "resource_profiles.json",
                json!({"scenarios":[{"scenario":"memory","samples":[]}]}),
            ),
            (
                "flamegraphs.json",
                json!([{"id":"ui_render_frame_profile","available":true,"benchmark_keys":["dispatch","unused"]},{"id":"missing","available":false}]),
            ),
        ];
        for (file, payload) in sources {
            std::fs::write(config.output_dir.join(file), payload.to_string())?;
            provenance::record(&config, file, provenance::timestamp(), &context, true)?;
        }
        let options = MeasureOptions {
            skip_bench: true,
            fail_on_slow: false,
            index_only: false,
        };
        speed_efficiency_report(&config, options)?;
        let path = config.output_dir.join("speed_efficiency_report.json");
        let report: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        assert_eq!(report["summary"]["measurement_status"], "valid");
        assert_eq!(report["summary"]["over_budget_latency"], 3);
        assert_eq!(report["summary"]["coverage_gaps"], 1);
        assert_eq!(
            report["sections"]["search_dispatch"][0]["family"],
            "search-dispatch"
        );
        assert_eq!(
            report["sections"]["tabs_and_splits"][0]["last_known_failure_ceiling"],
            "32 tabs"
        );
        assert_eq!(
            report["sections"]["flamegraph_coverage"][0]["covered_scenarios"],
            json!(["dispatch"])
        );
        assert_eq!(
            report["triage_summary"],
            json!({"critical":4,"ok":2,"unknown":0,"watch":0})
        );
        assert_eq!(report["triage"].as_array().unwrap().len(), 4);
        // A changed artifact must be excluded, not silently treated as fresh.
        std::fs::write(config.output_dir.join("slowspots.json"), "[]")?;
        speed_efficiency_report(&config, options)?;
        let stale: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        assert_eq!(stale["summary"]["measurement_status"], "incomplete");
        assert_eq!(stale["summary"]["tabs_and_splits_scenarios"], 0);
        Ok(())
    }
}
