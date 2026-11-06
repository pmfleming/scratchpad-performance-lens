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
        "cold_reads_during_save" => (
            3 * shared::GB,
            &[
                "database_bytes",
                "history_revisions",
                "cold_read_p99_ns",
                "physical_read_verified_count",
                "concurrent_saves",
            ],
        ),
        "aged_store_save" | "aged_store_restore" | "aged_store_close" | "aged_store_forget" => (
            3 * shared::GB,
            &["database_bytes", "history_revisions", "initial_archives"],
        ),
        "production_paste" => (128 * shared::MB, &["workflow_ns", "frame_p99_ns"]),
        "store_import" => (
            shared::GB,
            &["import_logical_bytes", "import_database_growth_bytes"],
        ),
        "file_backed_random_window" => (shared::GB, &["window_p99_ns", "window_samples"]),
        "file_backed_search" => (64 * shared::MB, &["elapsed_ns"]),
        "typing_without_save" => (50 * shared::MB, &["typing_p99_ns"]),
        "typing_during_save" => (50 * shared::MB, &["typing_p99_ns", "save_ns"]),
        "ui_during_store_writes" => (
            64 * shared::MB,
            &["frame_p99_ns", "concurrent_write_bytes", "seed_bytes"],
        ),
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
            &["retained_cache_bytes", "cache_limit_bytes", "result_value"],
        ),
        // Post-operation live bytes can be sampled after fixture destruction;
        // they cannot establish retention while the edited document is still alive.
        "provenance_retained_memory" => (
            100_000,
            &["retained_live_bytes_before_teardown", "baseline_live_bytes"],
        ),
        "session_persist_cost" | "session_steady_persist_cost" => (
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
        if !crate::measurement_contract::import_fixture_valid(event) {
            missing.push("non_deduplicated_import".into());
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

// Scale requires both complete resource evidence and a budget for this exact operation.
pub(super) fn demonstrated_workload(scenario: &str, samples: &[Value]) -> Option<i64> {
    let mut groups: std::collections::BTreeMap<i64, Vec<Value>> = std::collections::BTreeMap::new();
    for sample in samples {
        if let Some(workload) = sample["workload_value"].as_i64() {
            groups.entry(workload).or_default().push(sample.clone());
        }
    }
    groups
        .into_iter()
        .filter(|(_, group)| {
            super::resource_budgets::group_status(group) == "pass"
                && group.iter().all(|sample| {
                    gap_evidence(scenario, std::slice::from_ref(sample), false)["closed"] == true
                })
        })
        .map(|(workload, _)| workload)
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
        "cold_reads_during_save"
            if event["phase_elapsed_ns"]["physical_read_verified_count"]
                .as_u64()
                .unwrap_or(0)
                < 100
                || event["phase_elapsed_ns"]["concurrent_saves"]
                    .as_u64()
                    .unwrap_or(0)
                    == 0 =>
        {
            Some("verified_cold_reads_with_concurrent_saves")
        }
        id if (id.starts_with("aged_store_") || id == "cold_reads_during_save")
            && (event["workload_details"]["database_bytes"]
                .as_i64()
                .unwrap_or(0)
                < 3 * shared::GB
                || event["workload_details"]["history_revisions"]
                    .as_u64()
                    .unwrap_or(0)
                    < 128) =>
        {
            Some("aged_multi_gib_database")
        }
        "ui_during_store_writes"
            if event["phase_elapsed_ns"]["seed_bytes"]
                .as_i64()
                .unwrap_or(0)
                < 3 * shared::GB
                || event["phase_elapsed_ns"]["concurrent_write_bytes"]
                    .as_i64()
                    .unwrap_or(0)
                    < 3 * shared::GB =>
        {
            Some("real_writes_against_multi_gib_store")
        }
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
    crate::fixture_contract::matches(&event["workload_details"], &event["fixture_contract"])
        && event["measurement_scope"] == "prepared_operation"
}

pub(super) fn cache_bound_held(event: &Value) -> Option<bool> {
    let bytes = event["retained_cache_bytes"]
        .as_u64()
        .or_else(|| event["phase_elapsed_ns"]["retained_cache_bytes"].as_u64())?;
    let limit = event["cache_limit_bytes"]
        .as_u64()
        .or_else(|| event["phase_elapsed_ns"]["cache_limit_bytes"].as_u64())?;
    Some(bytes <= limit)
}

#[cfg(test)]
mod tests {
    use super::{expected_scenarios, gap_evidence};
    use crate::shared;
    use serde_json::{json, Value};
    #[test]
    fn chunk_counts_cannot_substitute_for_the_cache_byte_limit() {
        let mut sample = json!({"retained_file_chunks":17,"file_chunk_cache_limit":257,
            "phase_elapsed_ns":{"retained_cache_bytes":4 * shared::MB + 1,"cache_limit_bytes":4 * shared::MB}});
        assert_eq!(super::cache_bound_held(&sample), Some(false));
        sample["phase_elapsed_ns"]["retained_cache_bytes"] = json!(4 * shared::MB);
        assert_eq!(super::cache_bound_held(&sample), Some(true));
        sample["phase_elapsed_ns"] = Value::Null;
        assert_eq!(super::cache_bound_held(&sample), None);
    }
    #[test]
    fn cold_read_labels_require_physical_reads_and_concurrent_saves() {
        let mut row = json!({"status":"ok","elapsed_ns":100,"workload_value":3 * shared::GB,
            "measurement_scope":"prepared_operation",
            "measurement_contract":{"text_backing":"store","production_path":true,"code_path":"Chunk::load","fixture_kind":"non_repeating","fixture_revision":2},
            "workload_details":{"database_bytes":3 * shared::GB,"history_revisions":128},
            "phase_elapsed_ns":{"cold_read_p99_ns":3000000,"physical_read_verified_count":0,"concurrent_saves":3}});
        let check = |row: &Value| {
            gap_evidence("cold_reads_during_save", std::slice::from_ref(row), false)["closed"]
                == true
        };
        assert!(!check(&row));
        row["phase_elapsed_ns"]["physical_read_verified_count"] = json!(128);
        assert!(check(&row));
        row["phase_elapsed_ns"]["concurrent_saves"] = json!(0);
        assert!(!check(&row));
    }
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
            "fixture_contract":{"equals":{"fixture_revision":2,"history_retention":"unbounded","inserted_bytes":1,"synchronous":"FULL"},"minimum":{"history_payload_bytes":50 * shared::MB,"history_entries":1025}},
            "workload_details":{"fixture_revision":2,"history_retention":"unbounded","history_payload_bytes":50 * shared::MB,"history_entries":1025,"inserted_bytes":1,"synchronous":"FULL"}});
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
