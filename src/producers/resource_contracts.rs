//! A scenario name is an implementation label, not proof that a measurement gap is closed.
use crate::shared;
use serde_json::{json, Value};

pub(super) fn gap_evidence(scenario: &str, events: &[Value], synthetic: bool) -> Value {
    let (minimum, fields): (i64, &[&str]) = match scenario {
        "large_utf8_load_peak_memory" => (shared::GB, &["peak_live_bytes", "baseline_live_bytes", "working_set_bytes"]),
        "file_backed_chunk_cache_tracking" => (shared::GB, &["retained_file_chunks", "file_chunk_cache_limit", "result_value"]),
        "provenance_retained_memory" => (100_000, &["live_bytes", "baseline_live_bytes", "live_growth_bytes"]),
        "session_persist_cost" => (10_000, &["manifest_size_bytes", "snapshot_capture_ns", "file_write_ns", "manifest_serialize_ns"]),
        "session_restore_cost" => (10_000, &["manifest_read_parse_ns", "restore_reconstruction_ns"]),
        "startup_visible_restore_cost" => (10_000, &["startup_load_ns", "first_paint_ns"]),
        "tab_strip_frame_rendering" => (10_000, &["allocated_bytes", "peak_live_bytes"]),
        "many_file_lazy_open_tracking" => (10_000, &["allocated_bytes", "live_bytes"]),
        "search_app_result_tracking" => (1_000, &["allocated_bytes", "live_bytes"]),
        "edited_buffer_search_preview_rendering" => (10_000, &["allocated_bytes", "peak_live_bytes"]),
        "anchor_heavy_view_editing" => (1_000, &["allocated_bytes", "peak_live_bytes"]),
        "fragmented_long_session_mutation" => (10_000, &["allocated_bytes", "peak_live_bytes"]),
        _ => return json!({"status":"not_applicable", "closed":false}),
    };
    let mut missing = Vec::new();
    if synthetic { missing.push("real_non_synthetic_evidence".to_owned()); }
    if events.is_empty() { missing.push("samples".to_owned()); }
    if !events.iter().any(|event| event["workload_value"].as_i64().is_some_and(|value| value >= minimum)) {
        missing.push(format!("workload_at_least_{minimum}"));
    }
    for event in events {
        if event["status"] != "ok" { missing.push("successful_samples".into()); }
        if event["elapsed_ns"].as_u64().is_none_or(|value| value == 0) { missing.push("elapsed_ns".into()); }
        if event["measurement_scope"].as_str().is_none_or(|scope| scope.is_empty() || scope == "unknown") {
            missing.push("measurement_scope".into());
        }
        for field in fields {
            if event[*field].as_f64().is_none() && event["phase_elapsed_ns"][*field].as_f64().is_none() {
                missing.push((*field).to_owned());
            }
        }
        if scenario == "file_backed_chunk_cache_tracking"
            && event["retained_file_chunks"].as_u64().zip(event["file_chunk_cache_limit"].as_u64())
                .is_some_and(|(retained, limit)| retained > limit) {
            missing.push("cache_bound_compliance".into());
        }
    }
    missing.sort(); missing.dedup();
    json!({"status": if missing.is_empty() {"measured"} else {"incomplete"},
        "closed":missing.is_empty(), "minimum_workload":minimum,"required_metrics":fields,"missing_evidence":missing})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_only_synthetic_failed_and_missing_phase_measurements_do_not_close_gaps() {
        let sample = json!({"workload_value":10000,"status":"ok","elapsed_ns":10,
            "measurement_scope":"prepared_operation","allocated_bytes":20,"peak_live_bytes":30});
        assert_eq!(gap_evidence("tab_strip_frame_rendering", &[sample.clone()], false)["closed"], true);
        assert_eq!(gap_evidence("tab_strip_frame_rendering", &[sample.clone()], true)["closed"], false);
        assert_eq!(gap_evidence("session_persist_cost", &[sample.clone()], false)["closed"], false);
        let mut failed = sample; failed["status"] = json!("panic");
        assert_eq!(gap_evidence("tab_strip_frame_rendering", &[failed], false)["closed"], false);
    }
}
