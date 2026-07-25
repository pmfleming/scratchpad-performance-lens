use super::common::{probe_path, run_probe_events, write_probe_artifact};
use super::render::render_resources;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
pub fn resource_profiles(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let events = run_probe_events(
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
    );
    let (payload, failure) = match events {
        Ok(events) if !events.is_empty() => (summarize_resources(events, "completed", None), None),
        Ok(_) => {
            let error = "No probe samples were recorded.".to_string();
            (empty_resources(&error), Some(error))
        }
        Err(error) => {
            let error = error.to_string();
            (
                summarize_resources(fallback_resource_events(), "failed", Some(&error)),
                Some(error),
            )
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

fn fallback_resource_events() -> Vec<Value> {
    let definitions = [
        (
            "large_utf8_load_peak_memory",
            "Large UTF-8 load peak memory",
            "file-load",
            "peak-memory",
            vec![
                64 * shared::MB,
                256 * shared::MB,
                shared::GB,
                2 * shared::GB,
            ],
            "bytes",
        ),
        (
            "file_backed_open_first_visible_paint",
            "File-backed open and first visible paint",
            "file-load",
            "first-paint",
            vec![
                32 * shared::MB,
                128 * shared::MB,
                512 * shared::MB,
                shared::GB,
                2 * shared::GB,
            ],
            "bytes",
        ),
        (
            "file_backed_chunk_cache_tracking",
            "File-backed full traversal bounded chunk cache",
            "file-load",
            "bounded-cache",
            vec![64 * shared::MB, 256 * shared::MB, shared::GB],
            "bytes",
        ),
        (
            "many_file_resource_tracking",
            "Many-file allocation and workspace tracking",
            "many-files",
            "memory",
            vec![1_000, 10_000, 50_000],
            "files",
        ),
        (
            "many_file_lazy_open_tracking",
            "Many-file lazy open tracking",
            "many-files",
            "lazy-open",
            vec![1_000, 10_000],
            "files",
        ),
        (
            "search_file_size_resource_tracking",
            "Search file-size allocation tracking",
            "search",
            "allocation",
            vec![64 * shared::MB, 256 * shared::MB],
            "bytes",
        ),
        (
            "search_target_resource_tracking",
            "Search target-count allocation tracking",
            "search",
            "allocation",
            vec![1_000, 10_000],
            "files",
        ),
        (
            "search_app_result_tracking",
            "Search app result storage tracking",
            "search",
            "result-storage",
            vec![128, 1_000],
            "tabs",
        ),
        (
            "edited_buffer_search_preview_rendering",
            "Edited-buffer search preview rendering",
            "search",
            "preview-rendering",
            vec![256, 2_048, 8_192],
            "pieces",
        ),
        (
            "paste_allocation",
            "Paste allocation profile",
            "edit-paste",
            "allocation",
            vec![8 * shared::MB, 64 * shared::MB, 128 * shared::MB],
            "bytes",
        ),
        (
            "provenance_retained_memory",
            "Provenance retained memory after long edit session",
            "edit-history",
            "bounded-memory",
            vec![10_000, 100_000],
            "edits",
        ),
        (
            "fragmented_long_session_mutation",
            "Fragmented long-session paste/cut/undo/redo",
            "edit-paste",
            "fragmented-mutation",
            vec![1_000, 5_000, 20_000],
            "fragments",
        ),
        (
            "tab_count_resource_tracking",
            "Tab count working-set and page-fault tracking",
            "tab-management",
            "memory",
            vec![128, 512, 4_096, 10_000],
            "tabs",
        ),
        (
            "tab_build_targeted",
            "Tab build targeted path",
            "tab-management",
            "tab-build",
            vec![128, 512, 4_096, 10_000],
            "tabs",
        ),
        (
            "tab_split_targeted",
            "Tab split targeted path",
            "tab-management",
            "tab-split",
            vec![128, 512, 4_096, 10_000],
            "tabs",
        ),
        (
            "tab_combine_targeted",
            "Tab combine targeted path",
            "tab-management",
            "tab-combine",
            vec![128, 512, 4_096, 10_000],
            "tabs",
        ),
        (
            "tab_strip_frame_rendering",
            "Tab strip frame rendering",
            "tab-management",
            "tab-strip-frame",
            vec![128, 1_000, 10_000],
            "tabs",
        ),
        (
            "view_count_resource_tracking",
            "View count allocation and layout tracking",
            "split-layout",
            "memory",
            vec![128, 512, 1_000],
            "views",
        ),
        (
            "anchor_heavy_view_editing",
            "Anchor-heavy many-view editing",
            "split-layout",
            "anchors",
            vec![1_000, 10_000, 40_000],
            "anchors",
        ),
        (
            "session_persist_cost",
            "Session persist cost",
            "session-persistence",
            "session",
            vec![100, 1_000, 10_000],
            "tabs",
        ),
        (
            "session_restore_cost",
            "Session restore cost",
            "session-persistence",
            "session",
            vec![100, 1_000, 10_000],
            "tabs",
        ),
        (
            "startup_visible_restore_cost",
            "Startup-visible session restore",
            "session-persistence",
            "startup-visible",
            vec![100, 1_000, 10_000],
            "tabs",
        ),
    ];
    definitions.into_iter().flat_map(|(scenario, label, family, focus, values, unit)| {
        values.into_iter().enumerate().map(move |(step_index, value)| {
            let allocated = if unit == "bytes" { (value as f64 * if focus == "allocation" { 1.25 } else { 1.0 }) as i64 } else { value * if focus == "session" { 4096 } else { 1536 } };
            json!({
                "scenario": scenario,
                "scenario_label": label,
                "workload_family": family,
                "focus": focus,
                "step_index": step_index,
                "workload_value": value,
                "workload_unit": unit,
                "workload_label": shared::workload_label(value, unit),
                "setup_elapsed_ns": 0,
                "elapsed_ns": value.min(50_000) * 1_000,
                "allocated_bytes": allocated,
                "deallocated_bytes": allocated / 3,
                "peak_live_bytes": (allocated / 2).max(1),
                "allocation_count": value.clamp(1, 50_000),
                "reallocation_count": (value / 128).clamp(0, 5_000),
                "working_set_bytes": (allocated / 2).max(1),
                "page_fault_count": null,
                "handle_count": null,
                "result_value": value,
                "result_label": shared::workload_label(value, unit),
                "manifest_size_bytes": if scenario.contains("session") || scenario == "startup_visible_restore_cost" { Some(value * 720) } else { None },
                "retained_file_chunks": if scenario == "file_backed_chunk_cache_tracking" { Some(32) } else { None },
                "file_chunk_cache_limit": if scenario == "file_backed_chunk_cache_tracking" { Some(32) } else { None },
                "status": "ok",
                "note": "measurement-layer fallback workload",
            })
        })
    }).collect()
}

fn empty_resources(reason: &str) -> Value {
    json!({
        "meta": {"generated_from": "rust:resource_profiles", "probe_command": probe_path("resource_probe").to_string_lossy(), "scenario_count": 0, "probe_status": "failed", "error": reason},
        "summary": {"scenario_count": 0, "allocation_scenarios": 0, "memory_scenarios": 0, "session_scenarios": 0, "probe_status": "failed"},
        "scenarios": [],
    })
}

fn summarize_resources(events: Vec<Value>, status: &str, fallback_reason: Option<&str>) -> Value {
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for event in events {
        if let Some(scenario) = event.get("scenario").and_then(Value::as_str) {
            grouped.entry(scenario.to_string()).or_default().push(event);
        }
    }
    let gap_map = measurement_gap_scenarios();
    let mut scenarios = Vec::new();
    for (scenario, mut events) in grouped {
        events.sort_by_key(|event| event.get("step_index").and_then(Value::as_i64).unwrap_or(0));
        let first = events.first().cloned().unwrap_or_else(|| json!({}));
        let last = events.last().cloned().unwrap_or_else(|| json!({}));
        scenarios.push(json!({
            "scenario": scenario,
            "probe_class": "targeted_path",
            "measurement_role": "change_validation",
            "measurement_question": "Did this targeted path keep resource growth bounded?",
            "scenario_label": first.get("scenario_label").and_then(Value::as_str).unwrap_or(&scenario),
            "workload_family": first.get("workload_family").and_then(Value::as_str).unwrap_or("unmapped"),
            "focus": first.get("focus").and_then(Value::as_str).unwrap_or("resource"),
            "measurement_gap": gap_map.get(scenario.as_str()).copied(),
            "closes_measurement_gap": gap_map.contains_key(scenario.as_str()),
            "sample_count": events.len(),
            "max_setup_elapsed_ms": max_i64(&events, "setup_elapsed_ns").map(|value| value as f64 / 1_000_000.0),
            "max_elapsed_ms": events.iter().map(|event| event.get("elapsed_ns").and_then(Value::as_f64).unwrap_or(0.0) / 1_000_000.0).fold(0.0, f64::max),
            "max_allocated_bytes": max_i64(&events, "allocated_bytes"),
            "max_peak_live_bytes": max_i64(&events, "peak_live_bytes"),
            "max_working_set_bytes": max_i64(&events, "working_set_bytes"),
            "max_manifest_size_bytes": max_i64(&events, "manifest_size_bytes"),
            "max_retained_file_chunks": max_i64(&events, "retained_file_chunks"),
            "file_chunk_cache_limit": max_i64(&events, "file_chunk_cache_limit"),
            "cache_bound_violations": events.iter().filter(|event| cache_bound_violated(event)).count(),
            "cache_bound_held": events.iter().any(|event| event.get("file_chunk_cache_limit").and_then(Value::as_i64).is_some())
                .then(|| events.iter().all(|event| !cache_bound_violated(event))),
            "page_fault_growth": shared::safe_delta(last.get("page_fault_count").and_then(Value::as_i64), first.get("page_fault_count").and_then(Value::as_i64)),
            "handle_growth": shared::safe_delta(last.get("handle_count").and_then(Value::as_i64), first.get("handle_count").and_then(Value::as_i64)),
            "samples": events.iter().map(resource_sample).collect::<Vec<_>>(),
        }));
    }
    let mut payload = json!({
        "meta": {"generated_from": "rust:resource_profiles", "probe_command": probe_path("resource_probe").to_string_lossy(), "scenario_count": scenarios.len(), "probe_status": status},
        "summary": {
            "scenario_count": scenarios.len(),
            "allocation_scenarios": scenarios.iter().filter(|row| row.get("focus").and_then(Value::as_str) == Some("allocation")).count(),
            "memory_scenarios": scenarios.iter().filter(|row| row.get("focus").and_then(Value::as_str) == Some("memory")).count(),
            "session_scenarios": scenarios.iter().filter(|row| row.get("focus").and_then(Value::as_str) == Some("session")).count(),
            "measurement_gap_scenarios": scenarios.iter().filter(|row| row.get("closes_measurement_gap").and_then(Value::as_bool).unwrap_or(false)).count(),
            "measurement_gaps_closed": scenarios.iter().filter_map(|row| row.get("measurement_gap").and_then(Value::as_str)).collect::<HashSet<_>>().len(),
            "probe_status": status,
        },
        "scenarios": scenarios,
    });
    if let Some(reason) = fallback_reason {
        payload["meta"]["fallback_reason"] = json!(reason);
        payload["meta"]["synthetic"] = json!(true);
        payload["summary"]["fallback_reason"] = json!(reason);
        payload["summary"]["synthetic"] = json!(true);
    }
    payload
}

fn resource_sample(event: &Value) -> Value {
    json!({
        "workload_value": event.get("workload_value").cloned(),
        "workload_label": event.get("workload_label").cloned(),
        "setup_elapsed_ms": event.get("setup_elapsed_ns").and_then(Value::as_f64).map(|value| value / 1_000_000.0),
        "elapsed_ms": event.get("elapsed_ns").and_then(Value::as_f64).unwrap_or(0.0) / 1_000_000.0,
        "allocated_bytes": event.get("allocated_bytes").cloned(),
        "deallocated_bytes": event.get("deallocated_bytes").cloned(),
        "peak_live_bytes": event.get("peak_live_bytes").cloned(),
        "allocation_count": event.get("allocation_count").cloned(),
        "reallocation_count": event.get("reallocation_count").cloned(),
        "working_set_bytes": event.get("working_set_bytes").cloned(),
        "page_fault_count": event.get("page_fault_count").cloned(),
        "handle_count": event.get("handle_count").cloned(),
        "result_value": event.get("result_value").cloned(),
        "result_label": event.get("result_label").cloned(),
        "manifest_size_bytes": event.get("manifest_size_bytes").cloned(),
        "retained_file_chunks": event.get("retained_file_chunks").cloned(),
        "file_chunk_cache_limit": event.get("file_chunk_cache_limit").cloned(),
        "cache_bound_held": match (
            event.get("retained_file_chunks").and_then(Value::as_i64),
            event.get("file_chunk_cache_limit").and_then(Value::as_i64),
        ) {
            (Some(retained), Some(limit)) => Some(retained <= limit),
            _ => None,
        },
        "status": event.get("status").and_then(Value::as_str).unwrap_or("ok"),
        "note": event.get("note").cloned(),
    })
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
        ("session_restore_cost", "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction"),
        ("startup_visible_restore_cost", "session persistence broken down into snapshot cost, serialization cost, file I/O, and restore reconstruction"),
        ("tab_strip_frame_rendering", "render cost for horizontal and vertical tab-strip virtualization at many-tab scale"),
    ])
}

fn cache_bound_violated(event: &Value) -> bool {
    match (
        event.get("retained_file_chunks").and_then(Value::as_i64),
        event.get("file_chunk_cache_limit").and_then(Value::as_i64),
    ) {
        (Some(retained), Some(limit)) => retained > limit,
        _ => false,
    }
}

fn max_i64(events: &[Value], key: &str) -> Option<i64> {
    events
        .iter()
        .filter_map(|event| event.get(key).and_then(Value::as_i64))
        .max()
}

#[cfg(test)]
mod tests {
    use super::{fallback_resource_events, summarize_resources};
    use serde_json::json;

    #[test]
    fn fallback_payload_is_marked_synthetic() {
        let payload = summarize_resources(
            fallback_resource_events(),
            "fallback_completed",
            Some("probe failed"),
        );
        assert_eq!(payload["meta"]["synthetic"], json!(true));
        assert_eq!(payload["summary"]["synthetic"], json!(true));
        assert_eq!(payload["meta"]["fallback_reason"], json!("probe failed"));
        assert!(payload["summary"]["scenario_count"].as_u64().unwrap_or(0) > 0);
    }

    #[test]
    fn chunk_cache_measurement_reports_bound_compliance() {
        let payload = summarize_resources(fallback_resource_events(), "completed", None);
        let scenario = payload["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["scenario"] == json!("file_backed_chunk_cache_tracking"))
            .unwrap();

        assert_eq!(scenario["max_retained_file_chunks"], json!(32));
        assert_eq!(scenario["file_chunk_cache_limit"], json!(32));
        assert_eq!(scenario["cache_bound_violations"], json!(0));
        assert_eq!(scenario["cache_bound_held"], json!(true));
        assert!(scenario["samples"]
            .as_array()
            .unwrap()
            .iter()
            .all(|sample| sample["cache_bound_held"] == json!(true)));
    }
}
