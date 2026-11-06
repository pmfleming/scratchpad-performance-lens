use super::capacity_repetitions::aggregate as aggregate_capacity_repetitions;
use super::common::{probe_path, run_sweeps, values_array, write_probe_artifact};
use super::render::render_capacity;
use crate::artifacts::capacity_report::CapacityReportArtifact;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const CAPACITY_STRESS_FAMILY: &str = "capacity-stress";
const TEXT_LAYOUT_FAMILY: &str = "text-layout";

const FILE_SIZE_SWEEP: &[i64] = &[shared::MB, 64 * shared::MB, 256 * shared::MB, shared::GB];
const MANY_FILE_SWEEP: &[i64] = &[2_048, 10_000, 50_000];
const TEXT_LAYOUT_SWEEP: &[i64] = &[
    64 * 1024,
    shared::MB,
    4 * shared::MB,
    8 * shared::MB,
    16 * shared::MB,
    32 * shared::MB,
    64 * shared::MB,
    128 * shared::MB,
];
const SEARCH_TARGET_SWEEP: &[i64] = &[100, 1_000, 10_000];
const TAB_COUNT_SWEEP: &[i64] = &[512, 4_096, 10_000];
const SPLIT_COUNT_SWEEP: &[i64] = &[32, 128, 512, 1_000];
const VIEW_COUNT_SWEEP: &[i64] = &[128, 512, 1_000];
const PASTE_SIZE_SWEEP: &[i64] = &[8 * shared::MB, 64 * shared::MB, 128 * shared::MB];

struct CapacityScenarioSpec {
    id: &'static str,
    label: &'static str,
    fallback_values: &'static [i64],
    unit: &'static str,
    family: &'static str,
    profile_id: Option<&'static str>,
}

const CAPACITY_SCENARIOS: &[CapacityScenarioSpec] = &[
    CapacityScenarioSpec {
        id: "large_file_first_visible_ceiling",
        label: "Large-file first-visible window sweep",
        fallback_values: FILE_SIZE_SWEEP,
        unit: "bytes",
        family: "file-load",
        profile_id: Some("viewport_extraction_profile"),
    },
    CapacityScenarioSpec {
        id: "large_file_background_index_ceiling",
        label: "Large-file background indexing sweep",
        fallback_values: FILE_SIZE_SWEEP,
        unit: "bytes",
        family: "file-load",
        profile_id: Some("document_snapshot_profile"),
    },
    CapacityScenarioSpec {
        id: "in_memory_text_ingest_ceiling",
        label: "In-memory text ingest diagnostic sweep",
        fallback_values: FILE_SIZE_SWEEP,
        unit: "bytes",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: None,
    },
    CapacityScenarioSpec {
        id: "text_layout_ceiling",
        label: "Text Layout",
        fallback_values: TEXT_LAYOUT_SWEEP,
        unit: "bytes",
        family: TEXT_LAYOUT_FAMILY,
        profile_id: None,
    },
    CapacityScenarioSpec {
        id: "many_file_first_visible_ceiling",
        label: "Many-file first-visible workspace sweep",
        fallback_values: MANY_FILE_SWEEP,
        unit: "files",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: None,
    },
    CapacityScenarioSpec {
        id: "many_file_background_hydration_ceiling",
        label: "Many-file background hydration completion sweep",
        fallback_values: MANY_FILE_SWEEP,
        unit: "files",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: None,
    },
    CapacityScenarioSpec {
        id: "search_file_size_ceiling",
        label: "Search file-size ceiling sweep",
        fallback_values: FILE_SIZE_SWEEP,
        unit: "bytes",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("search_capacity_profile"),
    },
    CapacityScenarioSpec {
        id: "search_target_count_ceiling",
        label: "Search target-count ceiling sweep",
        fallback_values: SEARCH_TARGET_SWEEP,
        unit: "files",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("search_dispatch_profile"),
    },
    CapacityScenarioSpec {
        id: "tab_count_ceiling",
        label: "Tab count ceiling sweep",
        fallback_values: TAB_COUNT_SWEEP,
        unit: "tabs",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("tab_operations_profile"),
    },
    CapacityScenarioSpec {
        id: "split_count_ceiling",
        label: "Split count ceiling sweep",
        fallback_values: SPLIT_COUNT_SWEEP,
        unit: "splits",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("split_stress_profile"),
    },
    CapacityScenarioSpec {
        id: "view_count_ceiling",
        label: "View count ceiling sweep",
        fallback_values: VIEW_COUNT_SWEEP,
        unit: "views",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("view_navigation_profile"),
    },
    CapacityScenarioSpec {
        id: "paste_size_ceiling",
        label: "Paste size ceiling sweep",
        fallback_values: PASTE_SIZE_SWEEP,
        unit: "bytes",
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("paste_stress_profile"),
    },
];

pub fn capacity_report(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    capacity_report_for(config, &CAPACITY_SCENARIOS.iter().collect::<Vec<_>>())
}

pub fn promise_capacity_report(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let required = super::review::required_probe_scenarios(true);
    anyhow::ensure!(
        required.iter().all(|id| capacity_scenario(id).is_some()),
        "unknown required capacity scenario"
    );
    capacity_report_for(
        config,
        &CAPACITY_SCENARIOS
            .iter()
            .filter(|spec| required.contains(spec.id))
            .collect::<Vec<_>>(),
    )
}

fn capacity_report_for(config: &LensConfig, scenarios: &[&CapacityScenarioSpec]) -> Result<()> {
    let events = run_sweeps(
        &config.project_root,
        "capacity_probe",
        &scenarios.iter().map(|spec| spec.id).collect::<Vec<_>>(),
    );
    let (payload, failure) = match events {
        Ok(mut result) if !result.events.is_empty() => {
            let manifest: Value = serde_json::from_slice(&std::fs::read(
                config
                    .project_root
                    .join("benches/measurement_manifest.json"),
            )?)?;
            for event in &mut result.events {
                event["operation_budget"] =
                    manifest["capacity_budgets"][event["scenario"].as_str().unwrap_or("")].clone();
            }
            let validation = validate_capacity_events_for(&result.events, scenarios)
                .err()
                .map(|e| e.to_string());
            let mut payload = summarize_capacity(result.events, "completed");
            let failed = values_array(&payload["scenarios"])
                .iter()
                .flat_map(|row| values_array(&row["samples"]))
                .any(|sample| {
                    sample["complete"] != true || sample["failure_count"].as_u64().unwrap_or(0) > 0
                });
            let failure = result.failure.or(validation).or_else(|| {
                failed.then(|| "capacity contains incomplete or failed repetitions".to_owned())
            });
            payload["meta"]["error"] = json!(failure);
            if failure.is_some() {
                payload["meta"]["probe_status"] = json!("partial");
                payload["summary"]["probe_status"] = json!("partial");
            }
            (payload, failure)
        }
        Ok(_) => {
            let error = "No probe samples were recorded.".to_string();
            (empty_capacity(&error), Some(error))
        }
        Err(error) => {
            let error = error.to_string();
            (empty_capacity(&error), Some(error))
        }
    };
    let payload = serde_json::to_value(serde_json::from_value::<CapacityReportArtifact>(payload)?)?;
    write_probe_artifact(
        &config.output_dir.join("capacity_report.json"),
        &payload,
        "capacity report",
        render_capacity(&payload),
        failure.map(|error| format!("capacity probe failed: {error}")),
    )
}

fn capacity_config(scenario: &str) -> (&'static str, Option<&'static str>) {
    capacity_scenario(scenario).map_or((CAPACITY_STRESS_FAMILY, None), |spec| {
        (spec.family, spec.profile_id)
    })
}

fn capacity_scenario(scenario: &str) -> Option<&'static CapacityScenarioSpec> {
    CAPACITY_SCENARIOS.iter().find(|spec| spec.id == scenario)
}

#[cfg(test)]
fn validate_capacity_events(events: &[Value]) -> Result<()> {
    validate_capacity_events_for(events, &CAPACITY_SCENARIOS.iter().collect::<Vec<_>>())
}

fn validate_capacity_events_for(
    events: &[Value],
    scenarios: &[&CapacityScenarioSpec],
) -> Result<()> {
    for spec in scenarios {
        let matching: Vec<_> = events
            .iter()
            .filter(|event| {
                event["scenario"] == spec.id
                    || (spec.id == "text_layout_ceiling"
                        && event["scenario"] == "layout_bytes_ceiling")
            })
            .collect();
        anyhow::ensure!(
            !matching.is_empty(),
            "capacity probe omitted expected scenario {} ({})",
            spec.id,
            spec.label
        );
        anyhow::ensure!(
            matching
                .iter()
                .all(|event| event["workload_unit"] == spec.unit),
            "wrong workload unit for {}",
            spec.id
        );
        anyhow::ensure!(
            matching
                .iter()
                .filter_map(|event| event["workload_value"].as_i64())
                .max()
                >= spec.fallback_values.iter().copied().max(),
            "incomplete workload sweep for {}",
            spec.id
        );
    }
    Ok(())
}

fn empty_capacity(reason: &str) -> Value {
    json!({
        "meta": {"generated_from": "rust:capacity_report", "probe_command": probe_path("capacity_probe").to_string_lossy(), "scenario_count": 0, "probe_status": "failed", "error": reason, "synthetic": false},
        "summary": {"scenario_count": 0, "ceilings_reached": 0, "memory_bound_scenarios": 0, "cpu_bound_scenarios": 0, "probe_status": "failed", "synthetic": false},
        "scenarios": [],
    })
}

fn summarize_capacity(events: Vec<Value>, status: &str) -> Value {
    let probe_command = events
        .first()
        .and_then(|event| event["probe_executable"].as_str())
        .unwrap_or("cargo build --release --bin capacity_probe (executable unavailable)")
        .to_owned();
    let scenarios: Vec<_> = group_capacity_events(events)
        .into_iter()
        .map(|(scenario, events)| capacity_scenario_summary(scenario, events))
        .collect();
    let ceilings = scenarios
        .iter()
        .filter(|row| row["ceiling_reached"] == true)
        .count();
    let memory = scenarios
        .iter()
        .filter(|row| row["suspected_limiting_resource"].as_str() == Some("memory"))
        .count();
    let cpu = scenarios
        .iter()
        .filter(|row| row["suspected_limiting_resource"] == "cpu")
        .count();
    let unknown = scenarios
        .iter()
        .filter(|row| row["suspected_limiting_resource"] == "unknown")
        .count();
    json!({
        "meta": {"generated_from": "rust:capacity_report", "probe_command": probe_command, "scenario_count": scenarios.len(), "probe_status": status, "synthetic": false},
        "summary": {"scenario_count": scenarios.len(), "ceilings_reached": ceilings, "memory_bound_scenarios": memory, "cpu_bound_scenarios": cpu, "unknown_resource_scenarios": unknown, "probe_status": status, "synthetic": false},
        "scenarios": scenarios,
    })
}

fn group_capacity_events(events: Vec<Value>) -> BTreeMap<String, Vec<Value>> {
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for mut event in events {
        if event["scenario"].as_str() == Some("layout_bytes_ceiling") {
            event["scenario"] = json!("text_layout_ceiling");
            event["scenario_label"] = json!("Text Layout");
            event["workload_family"] = json!("text-layout");
        }
        if let Some(scenario) = event["scenario"].as_str() {
            grouped.entry(scenario.to_string()).or_default().push(event);
        }
    }
    grouped
}

fn capacity_scenario_summary(scenario: String, events: Vec<Value>) -> Value {
    let mut events = aggregate_capacity_repetitions(events);
    events.sort_by_key(|event| event["step_index"].as_i64().unwrap_or(0));
    let (family, profile_id) = capacity_config(&scenario);
    let threshold_ms = events
        .first()
        .and_then(|event| event["operation_budget"]["threshold_ms"].as_f64())
        .filter(|ms| ms.is_finite() && *ms > 0.0)
        .unwrap_or(0.0);
    let failure_index = events
        .iter()
        .position(|event| capacity_step_failed(event, threshold_ms));
    let first_failure = failure_index.and_then(|index| events.get(index));
    let last_success = events[..failure_index.unwrap_or(events.len())]
        .iter()
        .rev()
        .find(|event| {
            event["complete"] == true
                && event["status"] == "ok"
                && event["elapsed_ns"]
                    .as_f64()
                    .is_some_and(|ns| ns > 0.0 && ns / 1e6 <= threshold_ms)
        });
    let complete = events.iter().all(|event| event["complete"] == true);
    let first = events.first().unwrap_or(&Value::Null);
    let last = events.last().unwrap_or(&Value::Null);
    let limiting = infer_limiting_resource(&events);
    let matching = profile_id.map_or_else(
        || shared::matching_flamegraph_ids(&scenario),
        |id| vec![id.to_string()],
    );
    let peak = events
        .iter()
        .filter_map(|event| {
            event["peak_working_set_bytes"]
                .as_i64()
                .or_else(|| event["working_set_bytes"].as_i64())
        })
        .max();
    let scenario_label = first
        .get("scenario_label")
        .and_then(Value::as_str)
        .unwrap_or(&scenario);
    let workload_family = first
        .get("workload_family")
        .and_then(Value::as_str)
        .unwrap_or(family);
    json!({
        "scenario": scenario,
        "probe_class": "ceiling_health",
        "measurement_role": "promise_health",
        "measurement_question": "Does this promise still pass as workload size increases?",
        "scenario_label": scenario_label,
        "workload_family": workload_family,
        "threshold_ms": threshold_ms,
        "threshold_authoritative": threshold_ms > 0.0,
        "failure_mode": if first_failure.is_some_and(|event| event["failure_count"].as_u64().unwrap_or(0) > 0) { "operation_failed" } else if first_failure.is_some() { "unusable_latency" } else if !complete { "incomplete" } else { "not_reached" },
        "ceiling_reached": if first_failure.is_some() { Some(true) } else { complete.then_some(false) },
        "last_successful_workload": last_success.and_then(|row| row.get("workload_value")),
        "last_successful_label": last_success.and_then(|row| row.get("workload_label")),
        "first_failure_workload": first_failure.and_then(|row| row.get("workload_value")),
        "first_failure_label": first_failure.and_then(|row| row.get("workload_label")),
        "peak_working_set_bytes": peak,
        "working_set_growth_bytes": shared::safe_delta(last.get("working_set_bytes").and_then(Value::as_i64), first.get("working_set_bytes").and_then(Value::as_i64)),
        "page_fault_growth": shared::safe_delta(last.get("page_fault_count").and_then(Value::as_i64), first.get("page_fault_count").and_then(Value::as_i64)),
        "handle_growth": shared::safe_delta(last.get("handle_count").and_then(Value::as_i64), first.get("handle_count").and_then(Value::as_i64)),
        "first_saturated_resource": limiting,
        "suspected_limiting_resource": limiting,
        "matching_flamegraphs": matching,
        "diagnosis_guidance": diagnosis_guidance(&limiting, &matching),
        "resource_checklist": resource_checklist(&limiting, &events),
        "samples": events.iter().map(capacity_sample).collect::<Vec<_>>(),
    })
}

fn capacity_step_failed(event: &Value, threshold_ms: f64) -> bool {
    event["failure_count"]
        .as_u64()
        .is_some_and(|count| count > 0)
        || threshold_ms > 0.0
            && event["complete"] == true
            && event["elapsed_ns"].as_f64().unwrap_or(0.0) / 1_000_000.0 > threshold_ms
}

fn infer_limiting_resource(events: &[Value]) -> String {
    // Growth and elapsed time do not establish saturation or causality. Only an
    // explicit instrumented saturation observation can name a limiting resource.
    events
        .iter()
        .filter_map(|event| event["saturated_resource"].as_str())
        .find(|resource| matches!(*resource, "cpu" | "memory" | "i/o" | "os-handles"))
        .unwrap_or("unknown")
        .to_owned()
}

fn diagnosis_guidance(limiting: &str, matching: &[String]) -> Vec<String> {
    let mut guidance = Vec::new();
    match limiting {
        "unknown" => guidance.push("No saturation evidence: compare process CPU, RSS, I/O and queue counters before assigning a bottleneck.".into()),
        "memory" => guidance.push("Prefer allocation, working-set, or page-fault diagnostics before adding another CPU flamegraph.".to_string()),
        "os-handles" => guidance.push("Inspect handle counts, temp files, and other OS limits during the next stress run.".to_string()),
        _ if !matching.is_empty() => guidance.push(format!("Mapped CPU profile coverage: {}.", matching.join(", "))),
        _ => guidance.push("Compare the ceiling result against nearby targeted latency rows before adding another profile.".to_string()),
    }
    guidance.push("Use the USE checklist: utilization, saturation, and errors for CPU, memory, I/O, and OS resources.".to_string());
    guidance
}

fn resource_checklist(limiting: &str, events: &[Value]) -> Vec<Value> {
    let first = events.first().unwrap_or(&Value::Null);
    let last = events.last().unwrap_or(&Value::Null);
    vec![
        json!({"resource": "cpu", "status": if limiting == "cpu" { "focus" } else { "watch" }, "note": if limiting == "cpu" { "Latency rose before another resource clearly saturated." } else { "Capture a CPU flamegraph only if working-set growth stays modest." }}),
        json!({"resource": "memory", "status": if limiting == "memory" { "focus" } else { "watch" }, "note": format!("Working-set growth {}; page-fault delta {}.", shared::human_bytes(shared::safe_delta(last.get("working_set_bytes").and_then(Value::as_i64), first.get("working_set_bytes").and_then(Value::as_i64))), shared::safe_delta(last.get("page_fault_count").and_then(Value::as_i64), first.get("page_fault_count").and_then(Value::as_i64)).map_or("-".to_string(), |v| v.to_string()))}),
        json!({"resource": "i/o", "status": "scenario-dependent", "note": "First-visible file workloads include bounded disk reads; prepared mutation/layout sweeps remain in-memory."}),
        json!({"resource": "os-resources", "status": if limiting == "os-handles" { "focus" } else { "watch" }, "note": format!("Handle growth {}.", shared::safe_delta(last.get("handle_count").and_then(Value::as_i64), first.get("handle_count").and_then(Value::as_i64)).map_or("-".to_string(), |v| v.to_string()))}),
    ]
}

fn capacity_sample(event: &Value) -> Value {
    json!({
        "measurement_contract": event["measurement_contract"],
        "workload_value": event.get("workload_value"),
        "workload_label": event.get("workload_label"),
        "setup_elapsed_ms": event["setup_elapsed_ns"].as_f64().map(|value| value / 1_000_000.0),
        "elapsed_ms": event["elapsed_ns"].as_f64().map(|ns| ns / 1_000_000.0),
        "background_completion_ms": event["background_completion_ns"].as_f64().map(|value| value / 1_000_000.0),
        "completed_workload_value": event.get("completed_workload_value"),
        "elapsed_min_ms": event["elapsed_min_ns"].as_f64().map(|ns| ns / 1_000_000.0),
        "elapsed_max_ms": event["elapsed_max_ns"].as_f64().map(|ns| ns / 1_000_000.0),
        "complete": event["complete"].as_bool().unwrap_or(false),
        "failure_count": event["failure_count"].as_u64().unwrap_or(0),
        "expected_repetition_count": event["expected_repetition_count"].as_u64(),
        "successful_repetition_count": event["successful_repetition_count"].as_u64(),
        "raw_repetitions": values_array(&event["raw_repetitions"]),
        "statistical_claim": "successful-trial median; no population tail guarantee",
        "repetition_count": event["repetition_count"].as_u64().unwrap_or(1),
        "measurement_scope": event.get("measurement_scope"),
        "metrics": event.get("metrics"),
        "memory_budget": event.get("memory_budget"),
        "process_resources": event.get("process_resources"),
        "peak_working_set_bytes": event.get("peak_working_set_bytes"),
        "note": event.get("note"),
        "working_set_bytes": event.get("working_set_bytes"),
        "page_fault_count": event.get("page_fault_count"),
        "handle_count": event.get("handle_count"),
        "status": event["status"].as_str().unwrap_or("ok"),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        aggregate_capacity_repetitions, capacity_config, summarize_capacity,
        validate_capacity_events, CAPACITY_SCENARIOS,
    };
    use serde_json::json;

    #[test]
    fn missing_resource_counters_are_unknown_and_raw_counters_survive() {
        assert_eq!(
            super::infer_limiting_resource(&[json!({"elapsed_ns":999})]),
            "unknown"
        );
        let sample = super::capacity_sample(&json!({"elapsed_ns":10,
            "metrics":{"search_max_queue_depth":4}, "memory_budget":{"layout_bytes":100},
            "process_resources":{"cpu_elapsed_ms":2}}));
        let typed: crate::artifacts::capacity_report::CapacitySample =
            serde_json::from_value(sample).unwrap();
        assert_eq!(typed.metrics["search_max_queue_depth"], 4);
        assert_eq!(typed.process_resources["cpu_elapsed_ms"], 2);
    }

    #[test]
    fn repeated_capacity_steps_use_median_elapsed_time() {
        let rows = aggregate_capacity_repetitions(vec![
            json!({"step_index": 0, "repeat_index": 0, "status": "ok", "elapsed_ns": 900, "setup_elapsed_ns": 90, "background_completion_ns": 9_000}),
            json!({"step_index": 0, "repeat_index": 1, "status": "ok", "elapsed_ns": 100, "setup_elapsed_ns": 10, "background_completion_ns": 1_000}),
            json!({"step_index": 0, "repeat_index": 2, "status": "ok", "elapsed_ns": 300, "setup_elapsed_ns": 30, "background_completion_ns": 3_000}),
        ]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["elapsed_ns"], json!(300));
        assert_eq!(rows[0]["elapsed_min_ns"], json!(100));
        assert_eq!(rows[0]["elapsed_max_ns"], json!(900));
        assert_eq!(rows[0]["setup_elapsed_ns"], json!(30));
        assert_eq!(rows[0]["background_completion_ns"], json!(3_000));
        assert_eq!(rows[0]["repetition_count"], json!(3));
    }

    #[test]
    fn ceiling_health_distinguishes_incomplete_failed_slow_and_successful_trials() {
        for (count, status, elapsed, mode, reached) in [
            (1, "ok", 10, "incomplete", None),
            (3, "panic", 10, "operation_failed", Some(true)),
            (3, "ok", 150_000_000, "unusable_latency", Some(true)),
            (3, "ok", 10, "not_reached", Some(false)),
        ] {
            let events = (0..count).map(|repeat_index| json!({"scenario":"tab_count_ceiling", "operation_budget":{"threshold_ms":140}, "step_index":0,
                "repeat_index":repeat_index,"workload_value":10000,"elapsed_ns":elapsed,"status":status,"working_set_bytes":123})).collect();
            let payload = summarize_capacity(events, "completed");
            let row = &payload["scenarios"][0];
            assert!(row["samples"][0]["peak_working_set_bytes"].is_null());
            assert_eq!(row["peak_working_set_bytes"], 123);
            assert_eq!(row["failure_mode"], mode);
            assert_eq!(row["ceiling_reached"], json!(reached));
            assert_eq!(
                row["samples"][0]["failure_count"],
                if status == "panic" { 3 } else { 0 }
            );
            assert_eq!(row["samples"][0]["elapsed_ms"].is_null(), status == "panic");
            let typed: crate::artifacts::capacity_report::CapacityReportArtifact =
                serde_json::from_value(payload).unwrap();
            assert_eq!(
                typed.scenarios[0].demonstrated_workload(),
                (reached == Some(false)).then_some(10000)
            );
        }
    }

    #[test]
    fn registry_validates_units_workload_and_legacy_layout_alias() {
        let mut events: Vec<_> = CAPACITY_SCENARIOS.iter().map(|spec| {
            assert_eq!(capacity_config(spec.id), (spec.family, spec.profile_id));
            json!({"scenario":if spec.id == "text_layout_ceiling" { "layout_bytes_ceiling" } else { spec.id },
                "workload_value":spec.fallback_values.iter().max(),"workload_unit":spec.unit})
        }).collect();
        assert!(validate_capacity_events(&events).is_ok());
        let payload = summarize_capacity(events.clone(), "partial");
        assert!(payload["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["scenario"] == "text_layout_ceiling"));
        events[0]["workload_unit"] = json!("wrong");
        assert!(validate_capacity_events(&events).is_err());
        events[0]["workload_unit"] = json!(CAPACITY_SCENARIOS[0].unit);
        events[0]["workload_value"] = json!(0);
        assert!(validate_capacity_events(&events).is_err());
        events.remove(0);
        assert!(validate_capacity_events(&events).is_err());
    }

    #[test]
    fn focused_capacity_omits_only_diagnostics_and_still_requires_full_sweeps() {
        let required = super::super::review::required_probe_scenarios(true);
        let specs: Vec<_> = CAPACITY_SCENARIOS
            .iter()
            .filter(|s| required.contains(s.id))
            .collect();
        assert_eq!(specs.len(), required.len());
        assert!(!required.contains("text_layout_ceiling"));
        assert!(!required.contains("in_memory_text_ingest_ceiling"));
        let mut events: Vec<_> = specs
            .iter()
            .map(|spec| {
                json!({
                    "scenario":spec.id, "workload_unit":spec.unit,
                    "workload_value":spec.fallback_values.iter().max(),
                })
            })
            .collect();
        assert!(super::validate_capacity_events_for(&events, &specs).is_ok());
        assert!(validate_capacity_events(&events).is_err());
        events[0]["workload_value"] = json!(1);
        assert!(super::validate_capacity_events_for(&events, &specs).is_err());
        events.remove(0);
        assert!(super::validate_capacity_events_for(&events, &specs).is_err());
    }
}
