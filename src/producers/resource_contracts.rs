//! A scenario name is an implementation label, not proof that a measurement gap is closed.
use crate::shared;
use serde_json::{json, Value};

pub(super) fn expected_scenarios(root: &std::path::Path) -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    let declaration: Value = serde_json::from_slice(
        &std::fs::read(root.join("benches/measurement_manifest.json"))
            .context("missing project-owned measurement manifest")?,
    )?;
    let scenarios = declaration["probes"]["resource_probe"]
        .as_array()
        .context("missing resource_probe scenario declaration")?;
    let mut names = std::collections::HashSet::new();
    let mut expected = Vec::new();
    for scenario in scenarios {
        let name = scenario
            .as_str()
            .filter(|name| !name.is_empty())
            .context("invalid resource scenario ID")?;
        anyhow::ensure!(names.insert(name), "duplicate resource scenario ID: {name}");
        expected.push(name.to_owned());
    }
    anyhow::ensure!(!expected.is_empty(), "empty resource scenario declaration");
    Ok(expected)
}

pub(super) fn gap_evidence(scenario: &str, events: &[Value], synthetic: bool) -> Value {
    let (minimum, fields): (i64, &[&str]) = match scenario {
        "large_utf8_load_peak_memory" => (
            shared::GB,
            &[
                "peak_live_bytes",
                "baseline_live_bytes",
                "working_set_bytes",
            ],
        ),
        "file_backed_chunk_cache_tracking" => (
            shared::GB,
            &[
                "retained_file_chunks",
                "file_chunk_cache_limit",
                "result_value",
            ],
        ),
        // Post-operation live bytes can be sampled after fixture destruction;
        // they cannot establish retention while the edited document is still alive.
        "provenance_retained_memory" => (
            100_000,
            &["retained_live_bytes_before_teardown", "baseline_live_bytes"],
        ),
        "session_persist_cost" => (
            10_000,
            &[
                "manifest_size_bytes",
                "snapshot_capture_ns",
                "file_write_ns",
                "manifest_serialize_ns",
            ],
        ),
        "large_dirty_buffer_save" => (
            50 * shared::MB,
            &[
                "save_ns",
                "write_bytes",
                "history_payload_bytes",
                "history_entries",
            ],
        ),
        "session_restore_cost" => (
            10_000,
            &["manifest_read_parse_ns", "restore_reconstruction_ns"],
        ),
        "startup_visible_restore_cost" => (10_000, &["startup_load_ns", "first_paint_ns"]),
        "tab_strip_frame_rendering" => (10_000, &["allocated_bytes", "peak_live_bytes"]),
        "many_file_lazy_open_tracking" => (10_000, &["allocated_bytes", "live_bytes"]),
        "search_app_result_tracking" => (1_000, &["allocated_bytes", "live_bytes"]),
        "edited_buffer_search_preview_rendering" => {
            (10_000, &["allocated_bytes", "peak_live_bytes"])
        }
        "anchor_heavy_view_editing" => (1_000, &["allocated_bytes", "peak_live_bytes"]),
        "fragmented_long_session_mutation" => (10_000, &["allocated_bytes", "peak_live_bytes"]),
        _ => return json!({"status":"not_applicable", "closed":false}),
    };
    let mut missing = Vec::new();
    if synthetic {
        missing.push("real_non_synthetic_evidence".to_owned());
    }
    if events.is_empty() {
        missing.push("samples".to_owned());
    }
    if !events.iter().any(|event| {
        event["workload_value"]
            .as_i64()
            .is_some_and(|value| value >= minimum)
    }) {
        missing.push(format!("workload_at_least_{minimum}"));
    }
    for event in events {
        if !crate::measurement_contract::eligible(scenario, &event["measurement_contract"]) {
            missing.push("production_store_path".into());
        }
        missing.extend(
            sample_requirements(event, fields)
                .into_iter()
                .map(str::to_owned),
        );
        missing.extend(scenario_requirement(scenario, event).map(str::to_owned));
    }
    missing.sort();
    missing.dedup();
    json!({"status": if missing.is_empty() {"measured"} else {"incomplete"},
        "closed":missing.is_empty(), "minimum_workload":minimum,"required_metrics":fields,"missing_evidence":missing})
}

// Resource probes prove completed work and measurement completeness, not a latency budget.
// Latency budgets are evaluated separately from authoritative benchmark evidence.
pub(super) fn demonstrated_workload(scenario: &str, samples: &[Value]) -> Option<i64> {
    samples
        .iter()
        .filter(|sample| {
            gap_evidence(scenario, std::slice::from_ref(sample), false)["closed"] == true
        })
        .filter_map(|sample| sample["workload_value"].as_i64())
        .max()
}

fn sample_requirements<'a>(event: &Value, fields: &'a [&str]) -> Vec<&'a str> {
    let mut missing = Vec::new();
    if event["status"] != "ok" {
        missing.push("successful_samples");
    }
    if event["elapsed_ns"].as_u64().is_none_or(|value| value == 0) {
        missing.push("elapsed_ns");
    }
    if !matches!(
        event["measurement_scope"].as_str(),
        Some("prepared_operation" | "scenario_including_fixture_work")
    ) {
        missing.push("measurement_scope");
    }
    missing.extend(fields.iter().copied().filter(|field| {
        let value = [
            event,
            &event["phase_elapsed_ns"],
            &event["process_resources"],
            &event["workload_details"],
        ]
        .into_iter()
        .find_map(|source| source[*field].as_f64());
        value.is_none_or(|value| value < 0.0 || (field.ends_with("_ns") && value == 0.0))
    }));
    missing
}

fn scenario_requirement(scenario: &str, event: &Value) -> Option<&'static str> {
    match scenario {
        "large_dirty_buffer_save" if !full_history_save(event) => {
            Some("prepared_single_character_save_with_full_history")
        }
        "file_backed_chunk_cache_tracking" if cache_bound_held(event) == Some(false) => {
            Some("cache_bound_compliance")
        }
        _ => None,
    }
}

fn full_history_save(event: &Value) -> bool {
    let details = &event["workload_details"];
    details["history_payload_bytes"].as_i64().unwrap_or(0) >= 15 * shared::MB
        && details["history_entries"].as_u64().unwrap_or(0) >= 1024
        && details["inserted_bytes"] == 1
        && details["synchronous"] == "FULL"
        && event["measurement_scope"] == "prepared_operation"
}

pub(super) fn cache_bound_held(event: &Value) -> Option<bool> {
    Some(event["retained_file_chunks"].as_i64()? <= event["file_chunk_cache_limit"].as_i64()?)
}

#[cfg(test)]
mod tests {
    use super::{expected_scenarios, gap_evidence};
    use crate::shared;
    use serde_json::{json, Value};
    #[test]
    fn resource_expectations_come_from_project_owned_declarations() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("benches")).unwrap();
        let path = root.path().join("benches/measurement_manifest.json");
        for (scenarios, valid) in [
            (json!([]), false),
            (json!(["renamed", "renamed"]), false),
            (json!([null]), false),
            (json!(["large_file_first_visible_paint"]), true),
        ] {
            std::fs::write(
                &path,
                json!({"probes":{"resource_probe":scenarios}}).to_string(),
            )
            .unwrap();
            assert_eq!(expected_scenarios(root.path()).is_ok(), valid);
        }
    }

    #[test]
    fn large_save_requires_actual_write_counters_and_full_history() {
        let mut sample = json!({"workload_value":50 * shared::MB,"status":"ok","elapsed_ns":10,
            "measurement_scope":"prepared_operation", "phase_elapsed_ns":{"save_ns":9},
            "workload_details":{"history_payload_bytes":16 * shared::MB,"history_entries":1024,
                "inserted_bytes":1,"synchronous":"FULL"}});
        let check = |s: &Value| {
            gap_evidence("large_dirty_buffer_save", std::slice::from_ref(s), false)["closed"]
                == true
        };
        assert!(!check(&sample));
        sample["process_resources"] = json!({"write_bytes":4096});
        assert!(check(&sample));
        sample["workload_details"]["history_entries"] = json!(1);
        assert!(!check(&sample));
    }

    #[test]
    fn retention_requires_a_snapshot_before_document_destruction() {
        let mut sample = json!({"workload_value":100000,"status":"ok","elapsed_ns":10,
            "measurement_scope":"scenario_including_fixture_work", "baseline_live_bytes":100,
            "live_bytes":100, "live_growth_bytes":0});
        assert_eq!(
            gap_evidence(
                "provenance_retained_memory",
                std::slice::from_ref(&sample),
                false
            )["closed"],
            false
        );
        sample["retained_live_bytes_before_teardown"] = json!(1000);
        assert_eq!(
            gap_evidence(
                "provenance_retained_memory",
                std::slice::from_ref(&sample),
                false
            )["closed"],
            true
        );
    }

    #[test]
    fn name_only_synthetic_failed_and_missing_phase_measurements_do_not_close_gaps() {
        let sample = json!({"workload_value":10000,"status":"ok","elapsed_ns":10,
            "measurement_scope":"prepared_operation","allocated_bytes":20,"peak_live_bytes":30});
        assert_eq!(
            gap_evidence(
                "tab_strip_frame_rendering",
                std::slice::from_ref(&sample),
                false
            )["closed"],
            true
        );
        assert_eq!(
            gap_evidence(
                "tab_strip_frame_rendering",
                std::slice::from_ref(&sample),
                true
            )["closed"],
            false
        );
        assert_eq!(
            gap_evidence("session_persist_cost", std::slice::from_ref(&sample), false)["closed"],
            false
        );
        let mut failed = sample;
        failed["status"] = json!("panic");
        assert_eq!(
            gap_evidence("tab_strip_frame_rendering", &[failed], false)["closed"],
            false
        );
    }
}
