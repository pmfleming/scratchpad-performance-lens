use super::common::{probe_path, run_probe_events, write_probe_artifact};
use super::render::render_resources;
use super::resource_contracts::{cache_bound_held, expected_scenarios, gap_evidence};
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn resource_profiles(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let events = expected_scenarios(&config.project_root).and_then(|expected| {
        run_probe_events(
            &config.project_root,
            &[
                "cargo",
                "build",
                "--release",
                "--quiet",
                "--bin",
                "resource_probe",
            ],
            probe_path("resource_probe"),
            "resource probe",
        )
        .map(|events| (events, expected))
    });
    let (payload, failure) = match events {
        Ok((events, expected)) => validated_resources(events, &expected),
        Err(error) => {
            let error = error.to_string();
            (empty_resources(&error), Some(error))
        }
    };
    write_probe_artifact(
        &config.output_dir.join("resource_profiles.json"),
        &payload,
        "resource profiles",
        render_resources(&payload),
        failure.map(|error| format!("resource probe failed: {error}")),
    )
}

fn validated_resources(events: Vec<Value>, expected: &[String]) -> (Value, Option<String>) {
    let observed: HashSet<_> = events
        .iter()
        .filter_map(|event| event["scenario"].as_str())
        .collect();
    let missing: Vec<_> = expected
        .iter()
        .filter(|id| !observed.contains(id.as_str()))
        .collect();
    let failure = if !missing.is_empty() {
        Some(format!(
            "resource probe omitted expected scenarios: {}",
            missing
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    } else {
        events
            .iter()
            .any(|event| event["status"] != "ok")
            .then(|| "resource probe contains failed samples".to_owned())
    };
    // Valid samples from other scenarios remain evidence, even when a probe is incomplete.
    let mut payload = summarize_resources(
        events,
        if failure.is_some() {
            "partial"
        } else {
            "completed"
        },
    );
    payload["meta"]["missing_scenarios"] = json!(missing);
    payload["meta"]["expected_scenario_count"] = json!(expected.len());
    payload["meta"]["error"] = json!(failure);
    (payload, failure)
}

fn empty_resources(reason: &str) -> Value {
    json!({
        "meta": {"generated_from": "rust:resource_profiles", "probe_command": probe_path("resource_probe").to_string_lossy(), "scenario_count": 0, "probe_status": "failed", "error": reason},
        "summary": {"scenario_count": 0, "allocation_scenarios": 0, "memory_scenarios": 0, "session_scenarios": 0, "probe_status": "failed"},
        "scenarios": [],
    })
}

fn summarize_resources(events: Vec<Value>, status: &str) -> Value {
    let probe_command = events
        .first()
        .and_then(|event| event["probe_executable"].as_str())
        .unwrap_or("cargo build --release --bin resource_probe (executable unavailable)")
        .to_owned();
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for event in events {
        if let Some(scenario) = event["scenario"].as_str() {
            grouped.entry(scenario.to_owned()).or_default().push(event);
        }
    }
    let gap_map = measurement_gap_scenarios();
    let scenarios: Vec<_> = grouped
        .into_iter()
        .map(|(scenario, events)| {
            resource_scenario(&scenario, events, gap_map.get(scenario.as_str()).copied())
        })
        .collect();
    let closed_gaps = gap_map
        .values()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|gap| {
            gap_map
                .iter()
                .filter(|(_, description)| **description == *gap)
                .all(|(id, _)| {
                    scenarios
                        .iter()
                        .any(|row| row["scenario"] == *id && row["closes_measurement_gap"] == true)
                })
        })
        .count();
    json!({
        "meta": {"generated_from": "rust:resource_profiles", "probe_command": probe_command, "scenario_count": scenarios.len(), "probe_status": status},
        "summary": {
            "scenario_count": scenarios.len(),
            "allocation_scenarios": scenarios.iter().filter(|row| row["focus"] == "allocation").count(),
            "memory_scenarios": scenarios.iter().filter(|row| row["focus"] == "memory").count(),
            "session_scenarios": scenarios.iter().filter(|row| row["focus"] == "session").count(),
            "measurement_gap_scenarios": scenarios.iter().filter(|row| row["closes_measurement_gap"] == true).count(),
            "measurement_gaps_closed": closed_gaps,
            "probe_status": status,
        },
        "scenarios": scenarios,
    })
}

fn resource_scenario(scenario: &str, mut events: Vec<Value>, gap: Option<&str>) -> Value {
    events.sort_by_key(|event| event["step_index"].as_i64().unwrap_or(0));
    let first = events.first().unwrap_or(&Value::Null);
    let last = events.last().unwrap_or(&Value::Null);
    let evidence = gap_evidence(scenario, &events, false);
    json!({
        "scenario": scenario,
        "probe_class": "targeted_path",
        "measurement_role": "change_validation",
        "measurement_question": "Did this targeted path keep resource growth bounded?",
        "scenario_label": first["scenario_label"].as_str().unwrap_or(scenario),
        "workload_family": first["workload_family"].as_str().unwrap_or("unmapped"),
        "focus": first["focus"].as_str().unwrap_or("resource"),
        "measurement_gap": gap,
        "closes_measurement_gap": evidence["closed"],
        "gap_evidence": evidence,
        "sample_count": events.len(),
        "max_setup_elapsed_ms": max_i64(&events, "setup_elapsed_ns").map(|value| value as f64 / 1_000_000.0),
        "max_elapsed_ms": events.iter().map(|event| event["elapsed_ns"].as_f64().unwrap_or(0.0) / 1_000_000.0).fold(0.0, f64::max),
        "max_allocated_bytes": max_i64(&events, "allocated_bytes"),
        "max_peak_live_bytes": max_i64(&events, "peak_live_bytes"),
        "max_live_bytes": max_i64(&events, "live_bytes"),
        "max_retained_live_bytes_before_teardown": max_i64(&events, "retained_live_bytes_before_teardown"),
        "max_baseline_live_bytes": max_i64(&events, "baseline_live_bytes"),
        "max_live_growth_bytes": max_i64(&events, "live_growth_bytes"),
        "peak_working_set_bytes": max_i64(&events, "peak_working_set_bytes"),
        "peak_working_set_scope": "process_lifetime_high_water_not_operation_peak",
        "max_working_set_bytes": max_i64(&events, "working_set_bytes"),
        "max_manifest_size_bytes": max_i64(&events, "manifest_size_bytes"),
        "max_save_ms": events.iter().filter_map(|e| e["phase_elapsed_ns"]["save_ns"].as_f64()).reduce(f64::max).map(|ns| ns / 1_000_000.0),
        "max_write_bytes_per_operation": events.iter().filter_map(|e| e["process_resources"]["write_bytes"].as_u64()).max(),
        "max_retained_file_chunks": max_i64(&events, "retained_file_chunks"),
        "file_chunk_cache_limit": max_i64(&events, "file_chunk_cache_limit"),
        "cache_bound_violations": events.iter().filter(|event| cache_bound_held(event) == Some(false)).count(),
        "cache_bound_held": events.iter().any(|event| event["file_chunk_cache_limit"].as_i64().is_some())
            .then(|| events.iter().all(|event| cache_bound_held(event) != Some(false))),
        "page_fault_growth": shared::safe_delta(last["page_fault_count"].as_i64(), first["page_fault_count"].as_i64()),
        "handle_growth": shared::safe_delta(last["handle_count"].as_i64(), first["handle_count"].as_i64()),
        "samples": events.iter().map(resource_sample).collect::<Vec<_>>(),
    })
}

fn resource_sample(event: &Value) -> Value {
    // Preserve all evolving probe counters; only derived fields need new values.
    let mut sample = event.as_object().cloned().unwrap_or_default();
    for key in [
        "workload_value",
        "workload_label",
        "allocated_bytes",
        "deallocated_bytes",
        "peak_live_bytes",
        "allocation_count",
        "reallocation_count",
        "working_set_bytes",
        "page_fault_count",
        "handle_count",
        "result_value",
        "result_label",
        "manifest_size_bytes",
        "retained_file_chunks",
        "file_chunk_cache_limit",
        "note",
    ] {
        sample.entry(key).or_insert(Value::Null);
    }
    sample.insert(
        "setup_elapsed_ms".into(),
        json!(event["setup_elapsed_ns"].as_f64().map(|ns| ns / 1e6)),
    );
    sample.insert(
        "elapsed_ms".into(),
        json!(event["elapsed_ns"].as_f64().unwrap_or(0.0) / 1e6),
    );
    sample.insert("cache_bound_held".into(), json!(cache_bound_held(event)));
    sample.insert(
        "status".into(),
        json!(event["status"].as_str().unwrap_or("ok")),
    );
    Value::Object(sample)
}

fn measurement_gap_scenarios() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("large_utf8_load_peak_memory", "peak RSS / allocator high-water mark during very large UTF-8 load"),
        ("file_backed_chunk_cache_tracking", "bounded retained-memory behavior after traversing every chunk of a large file"),
        ("edited_buffer_search_preview_rendering", "edited-buffer search preview rendering with many matches and many pieces"),
        ("provenance_retained_memory", "provenance-store retained memory after hundreds of thousands of edits and history-budget eviction"),
        ("anchor_heavy_view_editing", "anchor-heavy editing with many views, selections, search results, and scroll anchors"),
        ("fragmented_long_session_mutation", "fragmented-buffer paste/cut/undo/redo after long sessions"),
        ("many_file_lazy_open_tracking", "large open batches should install path-backed shells and defer file content hydration"),
        ("search_app_result_tracking", "app-level search result storage should avoid eager matched-text allocation"),
        ("session_persist_cost", "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction"),
        ("large_dirty_buffer_save", "one-character autosave time and write bytes for a 50 MiB dirty buffer with full undo history"),
        ("session_restore_cost", "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction"),
        ("startup_visible_restore_cost", "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction"),
        ("tab_strip_frame_rendering", "render cost for horizontal and vertical tab-strip virtualization at many-tab scale"),
    ])
}

fn max_i64(events: &[Value], key: &str) -> Option<i64> {
    events.iter().filter_map(|event| event[key].as_i64()).max()
}

#[cfg(test)]
mod tests {
    use super::{summarize_resources, validated_resources};
    use serde_json::json;

    #[test]
    fn resource_validation_retains_real_samples_on_missing_or_failed_scenarios() {
        for (status, missing) in [("ok", false), ("ok", true), ("panic", false)] {
            let mut expected = vec!["first_paint".to_owned()];
            if missing {
                expected.push("missing_operation".into());
            }
            let (payload, failure) = validated_resources(
                vec![json!({"scenario":"first_paint",
                "status":status,"elapsed_ns":1_000_000,"workload_value":1_073_741_824,"step_index":0})],
                &expected,
            );
            assert_eq!(failure.is_some(), missing || status != "ok");
            assert_eq!(
                payload["meta"]["probe_status"],
                if failure.is_some() {
                    "partial"
                } else {
                    "completed"
                }
            );
            assert_eq!(
                payload["meta"]["missing_scenarios"],
                if missing {
                    json!(["missing_operation"])
                } else {
                    json!([])
                }
            );
            assert_eq!(payload["scenarios"][0]["samples"][0]["elapsed_ms"], 1.0);
            assert_ne!(payload["meta"]["synthetic"], true);
        }
    }

    #[test]
    fn resource_summary_preserves_scope_counters_and_cache_compliance() {
        for (retained, held) in [
            (json!(32), json!(true)),
            (json!(33), json!(false)),
            (json!(null), json!(null)),
        ] {
            let payload = summarize_resources(
                vec![json!({"scenario":"file_backed_chunk_cache_tracking",
                "elapsed_ns":10,"status":"ok","workload_value":shared::GB,"measurement_scope":"prepared_operation",
                "retained_file_chunks":retained,"file_chunk_cache_limit":32,"result_value":shared::GB,
                "live_bytes":100,"baseline_live_bytes":200,"live_growth_bytes":-100,
                "retained_live_bytes_before_teardown":350,"process_resources":{"read_bytes":1024}})],
                "completed",
            );
            let scenario = &payload["scenarios"][0];
            let sample = &scenario["samples"][0];
            assert_eq!(sample["cache_bound_held"], held);
            assert_eq!(
                scenario["cache_bound_violations"],
                usize::from(held == false)
            );
            assert_eq!(scenario["closes_measurement_gap"], held == true);
            assert_eq!(sample["retained_live_bytes_before_teardown"], 350);
            assert_eq!(sample["live_growth_bytes"], -100);
            assert_eq!(sample["measurement_scope"], "prepared_operation");
            assert_eq!(sample["process_resources"]["read_bytes"], 1024);
        }
    }
    use crate::shared;
}
