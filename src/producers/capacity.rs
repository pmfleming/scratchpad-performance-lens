use super::common::{probe_path, run_probe_events};
use super::render::render_capacity;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const DEFAULT_CAPACITY_THRESHOLD_MS: f64 = 180.0;
const CAPACITY_STRESS_FAMILY: &str = "capacity-stress";
const TEXT_LAYOUT_FAMILY: &str = "text-layout";

const FILE_SIZE_SWEEP: &[i64] = &[shared::MB, 64 * shared::MB, 256 * shared::MB, shared::GB];
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
const MANY_FILE_SWEEP: &[i64] = &[1_000, 10_000, 50_000];
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
    threshold_ms: f64,
    family: &'static str,
    profile_id: Option<&'static str>,
}

const CAPACITY_SCENARIOS: &[CapacityScenarioSpec] = &[
    CapacityScenarioSpec {
        id: "file_size_ceiling",
        label: "File size ceiling sweep",
        fallback_values: FILE_SIZE_SWEEP,
        unit: "bytes",
        threshold_ms: DEFAULT_CAPACITY_THRESHOLD_MS,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: None,
    },
    CapacityScenarioSpec {
        id: "text_layout_ceiling",
        label: "Text Layout",
        fallback_values: TEXT_LAYOUT_SWEEP,
        unit: "bytes",
        threshold_ms: DEFAULT_CAPACITY_THRESHOLD_MS,
        family: TEXT_LAYOUT_FAMILY,
        profile_id: None,
    },
    CapacityScenarioSpec {
        id: "many_file_count_ceiling",
        label: "Many-file workspace ceiling sweep",
        fallback_values: MANY_FILE_SWEEP,
        unit: "files",
        threshold_ms: DEFAULT_CAPACITY_THRESHOLD_MS,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: None,
    },
    CapacityScenarioSpec {
        id: "search_file_size_ceiling",
        label: "Search file-size ceiling sweep",
        fallback_values: FILE_SIZE_SWEEP,
        unit: "bytes",
        threshold_ms: DEFAULT_CAPACITY_THRESHOLD_MS,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("search_capacity_profile"),
    },
    CapacityScenarioSpec {
        id: "search_target_count_ceiling",
        label: "Search target-count ceiling sweep",
        fallback_values: SEARCH_TARGET_SWEEP,
        unit: "files",
        threshold_ms: DEFAULT_CAPACITY_THRESHOLD_MS,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("search_dispatch_profile"),
    },
    CapacityScenarioSpec {
        id: "tab_count_ceiling",
        label: "Tab count ceiling sweep",
        fallback_values: TAB_COUNT_SWEEP,
        unit: "tabs",
        threshold_ms: 140.0,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("tab_operations_profile"),
    },
    CapacityScenarioSpec {
        id: "split_count_ceiling",
        label: "Split count ceiling sweep",
        fallback_values: SPLIT_COUNT_SWEEP,
        unit: "splits",
        threshold_ms: 120.0,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("split_stress_profile"),
    },
    CapacityScenarioSpec {
        id: "view_count_ceiling",
        label: "View count ceiling sweep",
        fallback_values: VIEW_COUNT_SWEEP,
        unit: "views",
        threshold_ms: 120.0,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("view_navigation_profile"),
    },
    CapacityScenarioSpec {
        id: "paste_size_ceiling",
        label: "Paste size ceiling sweep",
        fallback_values: PASTE_SIZE_SWEEP,
        unit: "bytes",
        threshold_ms: 150.0,
        family: CAPACITY_STRESS_FAMILY,
        profile_id: Some("paste_stress_profile"),
    },
];

pub fn capacity_report(config: &LensConfig) -> Result<()> {
    let events = run_probe_events(
        &config.project_root,
        &[
            "cargo",
            "build",
            "--release",
            "--quiet",
            "--bin",
            "capacity_probe",
        ],
        probe_path("capacity_probe"),
        "capacity probe",
    );
    let payload = match events {
        Ok(events) if !events.is_empty() => summarize_capacity(events, "completed", None),
        Ok(_) => empty_capacity("No probe samples were recorded."),
        Err(error) => summarize_capacity(
            fallback_capacity_events(),
            "fallback_completed",
            Some(error.to_string()),
        ),
    };
    shared::write_visibility(
        &config.output_dir.join("capacity_report.json"),
        &payload,
        "capacity report",
        render_capacity(&payload),
    )
}

fn capacity_config(scenario: &str) -> (f64, &'static str, Option<&'static str>) {
    capacity_scenario(scenario).map_or(
        (DEFAULT_CAPACITY_THRESHOLD_MS, CAPACITY_STRESS_FAMILY, None),
        |spec| (spec.threshold_ms, spec.family, spec.profile_id),
    )
}

fn capacity_scenario(scenario: &str) -> Option<&'static CapacityScenarioSpec> {
    CAPACITY_SCENARIOS.iter().find(|spec| spec.id == scenario)
}

fn fallback_capacity_events() -> Vec<Value> {
    CAPACITY_SCENARIOS
        .iter()
        .flat_map(|spec| {
            spec.fallback_values
                .iter()
                .copied()
                .enumerate()
                .map(move |(step_index, value)| {
                    json!({
                        "scenario": spec.id,
                        "scenario_label": spec.label,
                        "workload_family": spec.family,
                        "step_index": step_index,
                        "workload_value": value,
                        "workload_unit": spec.unit,
                        "workload_label": shared::workload_label(value, spec.unit),
                        "elapsed_ns": (value.min(10_000) * 1_000),
                        "working_set_bytes": null,
                        "peak_working_set_bytes": null,
                        "page_fault_count": null,
                        "handle_count": null,
                        "status": "ok",
                        "note": "measurement-layer fallback workload",
                    })
                })
        })
        .collect()
}

fn empty_capacity(reason: &str) -> Value {
    json!({
        "meta": {"generated_from": "rust:capacity_report", "probe_command": probe_path("capacity_probe").to_string_lossy(), "scenario_count": 0, "probe_status": "failed", "error": reason},
        "summary": {"scenario_count": 0, "ceilings_reached": 0, "memory_bound_scenarios": 0, "cpu_bound_scenarios": 0, "probe_status": "failed"},
        "scenarios": [],
    })
}

fn summarize_capacity(events: Vec<Value>, status: &str, fallback_reason: Option<String>) -> Value {
    let synthetic = fallback_reason.is_some();
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for mut event in events {
        if event.get("scenario").and_then(Value::as_str) == Some("layout_bytes_ceiling") {
            event["scenario"] = json!("text_layout_ceiling");
            event["scenario_label"] = json!("Text Layout");
            event["workload_family"] = json!("text-layout");
        }
        if let Some(scenario) = event.get("scenario").and_then(Value::as_str) {
            grouped.entry(scenario.to_string()).or_default().push(event);
        }
    }
    let mut scenarios = Vec::new();
    for (scenario, mut events) in grouped {
        events.sort_by_key(|event| event.get("step_index").and_then(Value::as_i64).unwrap_or(0));
        let (threshold_ms, family, profile_id) = capacity_config(&scenario);
        let mut first_failure = None;
        let mut last_success = None;
        for event in &events {
            let elapsed_ms = event
                .get("elapsed_ns")
                .and_then(Value::as_f64)
                .unwrap_or(0.0)
                / 1_000_000.0;
            if event.get("status").and_then(Value::as_str) != Some("ok")
                || elapsed_ms > threshold_ms
            {
                first_failure = Some(event.clone());
                break;
            }
            last_success = Some(event.clone());
        }
        let first = events.first().cloned().unwrap_or_else(|| json!({}));
        let last = events.last().cloned().unwrap_or_else(|| json!({}));
        let limiting = infer_limiting_resource(&events);
        let matching = profile_id.map_or_else(
            || shared::matching_flamegraph_ids(&scenario),
            |id| vec![id.to_string()],
        );
        let peak = events
            .iter()
            .filter_map(|event| {
                event
                    .get("peak_working_set_bytes")
                    .or_else(|| event.get("working_set_bytes"))
                    .and_then(Value::as_i64)
            })
            .max();
        scenarios.push(json!({
            "scenario": scenario,
            "probe_class": "ceiling_health",
            "measurement_role": "promise_health",
            "measurement_question": "Does this promise still pass as workload size increases?",
            "scenario_label": first.get("scenario_label").and_then(Value::as_str).unwrap_or(&scenario),
            "workload_family": first.get("workload_family").and_then(Value::as_str).unwrap_or(family),
            "threshold_ms": threshold_ms,
            "failure_mode": if synthetic { "unmeasured" } else if first_failure.is_some() { "unusable_latency" } else { "not_reached" },
            "ceiling_reached": if synthetic { None } else { Some(first_failure.is_some()) },
            "last_successful_workload": last_success.as_ref().and_then(|row| row.get("workload_value")).cloned(),
            "last_successful_label": last_success.as_ref().and_then(|row| row.get("workload_label")).cloned(),
            "first_failure_workload": first_failure.as_ref().and_then(|row| row.get("workload_value")).cloned(),
            "first_failure_label": first_failure.as_ref().and_then(|row| row.get("workload_label")).cloned(),
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
        }));
    }
    let ceilings = scenarios
        .iter()
        .filter(|row| {
            row.get("ceiling_reached")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let memory = scenarios
        .iter()
        .filter(|row| {
            row.get("suspected_limiting_resource")
                .and_then(Value::as_str)
                == Some("memory")
        })
        .count();
    let mut payload = json!({
        "meta": {"generated_from": "rust:capacity_report", "probe_command": probe_path("capacity_probe").to_string_lossy(), "scenario_count": scenarios.len(), "probe_status": status},
        "summary": {"scenario_count": scenarios.len(), "ceilings_reached": ceilings, "memory_bound_scenarios": memory, "cpu_bound_scenarios": scenarios.len() - memory, "probe_status": status},
        "scenarios": scenarios,
    });
    if let Some(reason) = fallback_reason {
        payload["meta"]["fallback_reason"] = json!(reason);
        payload["meta"]["synthetic"] = json!(true);
        payload["summary"]["fallback_reason"] = payload["meta"]["fallback_reason"].clone();
        payload["summary"]["synthetic"] = json!(true);
    }
    payload
}

fn infer_limiting_resource(events: &[Value]) -> String {
    let Some(first) = events.first() else {
        return "cpu".to_string();
    };
    let Some(last) = events.last() else {
        return "cpu".to_string();
    };
    let handle_growth = shared::safe_delta(
        last.get("handle_count").and_then(Value::as_i64),
        first.get("handle_count").and_then(Value::as_i64),
    );
    let working_set_growth = shared::safe_delta(
        last.get("working_set_bytes").and_then(Value::as_i64),
        first.get("working_set_bytes").and_then(Value::as_i64),
    );
    let page_fault_growth = shared::safe_delta(
        last.get("page_fault_count").and_then(Value::as_i64),
        first.get("page_fault_count").and_then(Value::as_i64),
    );
    if handle_growth.is_some_and(|value| value >= 256) {
        "os-handles".to_string()
    } else if page_fault_growth.is_some_and(|value| value >= 10_000)
        || working_set_growth.is_some_and(|value| value >= 128 * shared::MB)
    {
        "memory".to_string()
    } else {
        "cpu".to_string()
    }
}

fn diagnosis_guidance(limiting: &str, matching: &[String]) -> Vec<String> {
    let mut guidance = Vec::new();
    match limiting {
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
    json!([
        {"resource": "cpu", "status": if limiting == "cpu" { "focus" } else { "watch" }, "note": if limiting == "cpu" { "Latency rose before another resource clearly saturated." } else { "Capture a CPU flamegraph only if working-set growth stays modest." }},
        {"resource": "memory", "status": if limiting == "memory" { "focus" } else { "watch" }, "note": format!("Working-set growth {}; page-fault delta {}.", shared::human_bytes(shared::safe_delta(last.get("working_set_bytes").and_then(Value::as_i64), first.get("working_set_bytes").and_then(Value::as_i64))), shared::safe_delta(last.get("page_fault_count").and_then(Value::as_i64), first.get("page_fault_count").and_then(Value::as_i64)).map_or("-".to_string(), |v| v.to_string()))},
        {"resource": "i/o", "status": "not-measured", "note": "These sweeps are in-memory. Re-run with file-backed workloads if open/save ceilings are the concern."},
        {"resource": "os-resources", "status": if limiting == "os-handles" { "focus" } else { "watch" }, "note": format!("Handle growth {}.", shared::safe_delta(last.get("handle_count").and_then(Value::as_i64), first.get("handle_count").and_then(Value::as_i64)).map_or("-".to_string(), |v| v.to_string()))}
    ])
    .as_array()
    .cloned()
    .unwrap_or_default()
}

fn capacity_sample(event: &Value) -> Value {
    json!({
        "workload_value": event.get("workload_value").cloned(),
        "workload_label": event.get("workload_label").cloned(),
        "elapsed_ms": event.get("elapsed_ns").and_then(Value::as_f64).unwrap_or(0.0) / 1_000_000.0,
        "working_set_bytes": event.get("working_set_bytes").cloned(),
        "page_fault_count": event.get("page_fault_count").cloned(),
        "handle_count": event.get("handle_count").cloned(),
        "status": event.get("status").and_then(Value::as_str).unwrap_or("ok"),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        capacity_config, fallback_capacity_events, summarize_capacity, CAPACITY_SCENARIOS,
        CAPACITY_STRESS_FAMILY, DEFAULT_CAPACITY_THRESHOLD_MS, TEXT_LAYOUT_FAMILY,
    };
    use serde_json::{json, Value};

    #[test]
    fn capacity_config_comes_from_registry() {
        assert_eq!(
            capacity_config("tab_count_ceiling"),
            (
                140.0,
                CAPACITY_STRESS_FAMILY,
                Some("tab_operations_profile")
            )
        );
        assert_eq!(
            capacity_config("text_layout_ceiling"),
            (DEFAULT_CAPACITY_THRESHOLD_MS, TEXT_LAYOUT_FAMILY, None)
        );
    }

    #[test]
    fn fallback_payload_is_marked_synthetic() {
        let payload = summarize_capacity(
            fallback_capacity_events(),
            "fallback_completed",
            Some("probe failed".to_string()),
        );
        assert_eq!(payload["meta"]["synthetic"], json!(true));
        assert_eq!(payload["summary"]["synthetic"], json!(true));
        assert!(payload["scenarios"].as_array().unwrap().iter().all(|row| {
            row.get("failure_mode").and_then(Value::as_str) == Some("unmeasured")
                && row.get("ceiling_reached").is_some_and(Value::is_null)
        }));
        assert_eq!(
            payload["summary"]["scenario_count"].as_u64(),
            Some(CAPACITY_SCENARIOS.len() as u64)
        );
    }
}
