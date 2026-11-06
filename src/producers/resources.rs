use super::common::{probe_path, run_sweeps, write_probe_artifact};
use super::render::render_resources;
use super::resource_contracts::{
    cache_bound_held, demonstrated_workload, expected_scenarios, gap_evidence,
};
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub fn resource_profiles(config: &LensConfig, options: MeasureOptions) -> Result<()> {
    resource_profiles_for(config, false, &options.scenarios)
}

pub fn promise_resource_profiles(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    resource_profiles_for(config, true, &[])
}

fn resource_profiles_for(
    config: &LensConfig,
    promises_only: bool,
    requested: &[String],
) -> Result<()> {
    let events = expected_scenarios(&config.project_root).and_then(|expected| {
        let expected = if promises_only {
            let required = super::review::required_probe_scenarios(false);
            anyhow::ensure!(
                required
                    .iter()
                    .all(|id| expected.iter().any(|name| name == id)),
                "missing required resource scenario declaration"
            );
            expected
                .into_iter()
                .filter(|id| required.contains(id.as_str()))
                .collect()
        } else {
            expected
        };
        let expected = select_scenarios(expected, requested)?;
        run_sweeps(
            &config.project_root,
            "resource_probe",
            &expected.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .and_then(|mut result| {
            super::resource_budgets::attach(&config.project_root, &mut result.events)?;
            Ok((result, expected))
        })
    });
    let (payload, failure) = match events {
        Ok((result, expected)) => {
            let (mut payload, failure) = validated_resources(result.events, &expected);
            payload["meta"]["selection_scope"] = json!(if requested.is_empty() {
                "complete"
            } else {
                "focused"
            });
            payload["meta"]["requested_scenarios"] = json!(requested);
            let failure = result.failure.or(failure);
            if failure.is_some() {
                payload["meta"]["probe_status"] = json!("partial");
                payload["summary"]["probe_status"] = json!("partial");
                payload["meta"]["error"] = json!(failure);
            }
            (payload, failure)
        }
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

fn select_scenarios(expected: Vec<String>, requested: &[String]) -> Result<Vec<String>> {
    if requested.is_empty() {
        return Ok(expected);
    }
    anyhow::ensure!(
        requested.iter().all(|id| expected.contains(id)),
        "unknown resource scenario selection"
    );
    Ok(expected
        .into_iter()
        .filter(|id| requested.contains(id))
        .collect())
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

pub(super) fn summarize_resources(events: Vec<Value>, status: &str) -> Value {
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
        "demonstrated_workload": demonstrated_workload(scenario, &events),
        "scale_evidence_basis": "successful_resource_contract_with_project_operation_budget",
        "sample_count": events.len(),
        "phase_diagnostics": super::resource_diagnostics::phases(&events),
        "budget_status": super::resource_budgets::group_status(&events),
        "budget_misses": events.iter().filter(|event| super::resource_budgets::status(event) == "fail").count(),
        "unbudgeted_samples": events.iter().filter(|event| super::resource_budgets::status(event) == "unbudgeted").count(),
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
        "max_write_amplification": events.iter().filter_map(write_amplification).reduce(f64::max),
        "max_retained_file_chunks": max_i64(&events, "retained_file_chunks"),
        "file_chunk_cache_limit": max_i64(&events, "file_chunk_cache_limit"),
        "cache_bound_violations": events.iter().filter(|event| cache_bound_held(event) == Some(false)).count(),
        "cache_bound_held": events.iter().any(|event| cache_bound_held(event).is_some())
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
    sample.insert(
        "write_amplification".into(),
        json!(write_amplification(event)),
    );
    sample.insert(
        "write_amplification_basis".into(),
        json!(
            if event["phase_elapsed_ns"]["persist_logical_bytes"].is_number() {
                "process_write_bytes_per_captured_live_text_byte_not_changed_bytes"
            } else {
                "process_write_bytes_per_imported_or_inserted_text_byte"
            }
        ),
    );
    sample.insert(
        "fixture_valid".into(),
        json!(crate::measurement_contract::import_fixture_valid(event)),
    );
    sample.insert("cache_bound_held".into(), json!(cache_bound_held(event)));
    sample.insert(
        "status".into(),
        json!(event["status"].as_str().unwrap_or("ok")),
    );
    Value::Object(sample)
}

fn write_amplification(event: &Value) -> Option<f64> {
    let logical = event["phase_elapsed_ns"]["persist_logical_bytes"]
        .as_f64()
        .or_else(|| event["phase_elapsed_ns"]["import_logical_bytes"].as_f64())
        .or_else(|| event["workload_details"]["inserted_bytes"].as_f64())?;
    let physical = event["process_resources"]["write_bytes"].as_f64()?;
    (logical.is_finite() && logical > 0.0 && physical.is_finite() && physical >= 0.0)
        .then_some(physical / logical)
}

fn measurement_gap_scenarios() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("production_paste", "production paste through publication"),
        ("cold_reads_during_save", "verified cold reads during concurrent saves"),
        ("aged_store_save", "save on an aged multi-GiB database"),
        ("aged_store_restore", "restore on an aged multi-GiB database"),
        ("aged_store_close", "close on an aged multi-GiB database"),
        ("aged_store_forget", "forget on an aged multi-GiB database"),
        ("store_import", "store import cost and materialized body bytes"),
        ("file_backed_random_window", "store read latency"),
        ("file_backed_search", "store search completion"),
        ("typing_without_save", "typing baseline"),
        ("typing_during_save", "typing while saves run"),
        ("ui_during_store_writes", "UI responsiveness during heavy store writes"),
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
mod selection_tests {
    #[test]
    fn persistence_write_ratio_requires_known_positive_logical_bytes() {
        use serde_json::json;
        let mut event = json!({"phase_elapsed_ns":{"persist_logical_bytes":100},"process_resources":{"write_bytes":2000}});
        assert_eq!(super::write_amplification(&event), Some(20.0));
        event["phase_elapsed_ns"]["persist_logical_bytes"] = json!(0);
        assert_eq!(super::write_amplification(&event), None);
        event["phase_elapsed_ns"]["persist_logical_bytes"] = json!(100);
        event["process_resources"]["write_bytes"] = serde_json::Value::Null;
        assert_eq!(super::write_amplification(&event), None);
    }
    #[test]
    fn focused_selection_is_exact_deduplicated_and_rejects_typos() {
        let expected = vec!["one".into(), "two".into(), "three".into()];
        let selected =
            super::select_scenarios(expected.clone(), &["two".into(), "two".into()]).unwrap();
        assert_eq!(selected, vec!["two"]);
        assert!(super::select_scenarios(expected.clone(), &["typo".into()]).is_err());
        assert_eq!(
            super::select_scenarios(expected.clone(), &[]).unwrap(),
            expected
        );
    }
}
