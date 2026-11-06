//! A scenario name is an implementation label, not proof that a measurement gap is closed.
use crate::shared::{GB, MB};
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

type GapContract = (&'static [&'static str], i64, &'static [&'static str]);
const GAP_CONTRACTS: &[GapContract] = &[
    (
        &["cold_reads_during_save"],
        3 * GB,
        &[
            "database_bytes",
            "history_revisions",
            "cold_read_p99_ns",
            "physical_read_verified_count",
            "concurrent_saves",
        ],
    ),
    (
        &[
            "aged_store_save",
            "aged_store_restore",
            "aged_store_close",
            "aged_store_forget",
        ],
        3 * GB,
        &["database_bytes", "history_revisions", "initial_archives"],
    ),
    (
        &["production_paste"],
        128 * MB,
        &["publication_ns", "workflow_ns", "frame_p99_ns"],
    ),
    (
        &["store_import"],
        GB,
        &["import_logical_bytes", "import_database_growth_bytes"],
    ),
    (
        &["file_backed_random_window"],
        GB,
        &["window_p99_ns", "window_samples"],
    ),
    (&["file_backed_search"], 64 * MB, &["elapsed_ns"]),
    (&["typing_without_save"], 50 * MB, &["typing_p99_ns"]),
    (
        &["typing_during_save"],
        50 * MB,
        &["typing_p99_ns", "save_ns"],
    ),
    (
        &["ui_during_store_writes"],
        64 * MB,
        &["frame_p99_ns", "concurrent_write_bytes", "seed_bytes"],
    ),
    (
        &["large_utf8_load_peak_memory"],
        GB,
        &[
            "peak_live_bytes",
            "baseline_live_bytes",
            "working_set_bytes",
        ],
    ),
    (
        &["file_backed_chunk_cache_tracking"],
        GB,
        &["retained_cache_bytes", "cache_limit_bytes", "result_value"],
    ),
    (
        &["provenance_retained_memory"],
        100_000,
        &["retained_live_bytes_before_teardown", "baseline_live_bytes"],
    ),
    (
        &["session_persist_cost", "session_steady_persist_cost"],
        10_000,
        &[
            "manifest_size_bytes",
            "snapshot_capture_ns",
            "file_write_ns",
            "manifest_serialize_ns",
        ],
    ),
    (
        &["large_dirty_buffer_save"],
        50 * MB,
        &[
            "save_ns",
            "write_bytes",
            "history_payload_bytes",
            "history_entries",
        ],
    ),
    (
        &["session_restore_cost"],
        10_000,
        &["manifest_read_parse_ns", "restore_reconstruction_ns"],
    ),
    (
        &["startup_visible_restore_cost"],
        10_000,
        &["startup_load_ns", "first_paint_ns"],
    ),
    (
        &[
            "tab_strip_frame_rendering",
            "edited_buffer_search_preview_rendering",
            "fragmented_long_session_mutation",
        ],
        10_000,
        &["allocated_bytes", "peak_live_bytes"],
    ),
    (
        &["many_file_lazy_open_tracking"],
        10_000,
        &["allocated_bytes", "live_bytes"],
    ),
    (
        &["search_app_result_tracking"],
        1_000,
        &["allocated_bytes", "live_bytes"],
    ),
    (
        &["anchor_heavy_view_editing"],
        1_000,
        &["allocated_bytes", "peak_live_bytes"],
    ),
];
fn gap_contract(scenario: &str) -> Option<(i64, &'static [&'static str])> {
    GAP_CONTRACTS
        .iter()
        .find(|(ids, _, _)| ids.contains(&scenario))
        .map(|(_, minimum, fields)| (*minimum, *fields))
}

pub(super) fn gap_evidence(scenario: &str, events: &[Value], synthetic: bool) -> Value {
    let Some((minimum, fields)) = gap_contract(scenario) else {
        return json!({"status":"not_applicable", "closed":false});
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
    let mut groups: std::collections::BTreeMap<i64, Vec<&Value>> =
        std::collections::BTreeMap::new();
    for sample in samples {
        if let Some(workload) = sample["workload_value"].as_i64() {
            groups.entry(workload).or_default().push(sample);
        }
    }
    groups
        .into_iter()
        .filter(|(_, group)| {
            super::resource_budgets::group_status(group.iter().copied()) == "pass"
                && group.iter().all(|sample| {
                    gap_evidence(scenario, std::slice::from_ref(*sample), false)["closed"] == true
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
        "aged_store_forget"
            if event["workload_details"]["fixture_revision"] != 2
                || event["workload_details"]["edit_preparation"]
                    != "recorded_single_character_insert" =>
        {
            Some("recorded_incremental_edit_before_forget")
        }
        "production_paste" if !published_paste(&event["phase_elapsed_ns"]) => {
            Some("published_paste_and_queued_input_with_separate_flush")
        }
        "cold_reads_during_save"
            if !verified_cold_reads(&event["phase_elapsed_ns"])
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
                < 3 * GB
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
                < 3 * GB
                || event["phase_elapsed_ns"]["concurrent_write_bytes"]
                    .as_i64()
                    .unwrap_or(0)
                    < 3 * GB =>
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

fn published_paste(phase: &Value) -> bool {
    let Some(publication) = phase["publication_ns"].as_u64().filter(|value| *value > 0) else {
        return false;
    };
    let Some(workflow) = phase["workflow_ns"]
        .as_u64()
        .filter(|value| *value >= publication)
    else {
        return false;
    };
    phase["publication_endpoint"] == "published_text_and_queued_input_tessellated"
        && phase["queued_typing_preserved"] == true
        && phase["writer_pause_ms"].as_u64() == Some(0)
        && phase["post_publication_flush_ns"]
            .as_u64()
            .is_some_and(|flush| flush <= workflow - publication)
        && phase["frame_p99_ns"]
            .as_u64()
            .is_some_and(|frame| frame > 0 && frame <= publication)
}

fn verified_cold_reads(phase: &Value) -> bool {
    if phase["physical_io_scope"] != "foreground_reader_thread_read_bytes" {
        return false;
    }
    let Some(samples) = phase["cold_read_samples"].as_array() else {
        return false;
    };
    if phase["read_sample_count"].as_u64() != Some(samples.len() as u64) {
        return false;
    }
    let mut verified = Vec::new();
    for sample in samples {
        let Some(elapsed) = sample["elapsed_ns"].as_u64().filter(|value| *value > 0) else {
            return false;
        };
        let Some(claimed) = sample["physical_read_verified"].as_bool() else {
            return false;
        };
        let observed = sample["eviction_succeeded"] == true
            && sample["read_bytes"].as_u64().is_some_and(|bytes| bytes > 0);
        if claimed != observed {
            return false;
        }
        if claimed {
            verified.push(elapsed);
        }
    }
    if verified.len() < 100
        || phase["physical_read_verified_count"].as_u64() != Some(verified.len() as u64)
    {
        return false;
    }
    verified.sort_unstable();
    phase["cold_read_p99_ns"].as_u64() == Some(verified[(verified.len() * 99).div_ceil(100) - 1])
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
    use super::{cache_bound_held, expected_scenarios, gap_evidence};
    use crate::shared;
    use serde_json::{json, Value};
    fn proven(id: &str, row: &Value) -> bool {
        gap_evidence(id, std::slice::from_ref(row), false)["closed"] == true
    }

    #[test]
    fn paste_proof_separates_publication_from_flush_and_requires_queued_input() {
        let row = json!({"status":"ok","elapsed_ns":100,"workload_value":128 * shared::MB,
            "measurement_scope":"prepared_operation",
            "measurement_contract":{"text_backing":"store","production_path":true,"code_path":"production input","fixture_kind":"non_repeating","fixture_revision":2},
            "phase_elapsed_ns":{"publication_ns":30,"workflow_ns":80,"frame_p99_ns":10,
                "post_publication_flush_ns":40,"writer_pause_ms":0,"queued_typing_preserved":true,
                "publication_endpoint":"published_text_and_queued_input_tessellated"}});
        assert!(proven("production_paste", &row));
        for (field, value) in [
            ("publication_ns", Value::Null),
            ("publication_ns", json!(90)),
            ("post_publication_flush_ns", json!(60)),
            ("writer_pause_ms", json!(200)),
            ("queued_typing_preserved", json!(false)),
            ("publication_endpoint", json!("flush_complete")),
        ] {
            let mut invalid = row.clone();
            invalid["phase_elapsed_ns"][field] = value;
            assert!(!proven("production_paste", &invalid), "{field}");
        }
    }

    #[test]
    fn forget_proof_requires_a_recorded_incremental_edit() {
        let mut row = json!({"status":"ok","elapsed_ns":100,"workload_value":3 * shared::GB,
            "measurement_scope":"prepared_operation",
            "measurement_contract":{"text_backing":"store","production_path":true,"code_path":"forget","fixture_kind":"non_repeating","fixture_revision":2},
            "workload_details":{"database_bytes":3 * shared::GB,"history_revisions":128,"initial_archives":128,
                "fixture_revision":2,"edit_preparation":"recorded_single_character_insert"}});
        assert!(proven("aged_store_forget", &row));
        row["workload_details"]["edit_preparation"] = json!("whole_document_replacement");
        assert!(!proven("aged_store_forget", &row));
        row["workload_details"]["edit_preparation"] = json!("recorded_single_character_insert");
        row["workload_details"]["fixture_revision"] = json!(1);
        assert!(!proven("aged_store_forget", &row));
    }

    #[test]
    fn store_proof_requires_byte_bounds_physical_reads_and_concurrent_saves() {
        let mut cache = json!({"retained_file_chunks":17,"file_chunk_cache_limit":257,
            "phase_elapsed_ns":{"retained_cache_bytes":4 * shared::MB + 1,"cache_limit_bytes":4 * shared::MB}});
        assert_eq!(cache_bound_held(&cache), Some(false));
        cache["phase_elapsed_ns"]["retained_cache_bytes"] = json!(4 * shared::MB);
        assert_eq!(cache_bound_held(&cache), Some(true));
        cache["phase_elapsed_ns"] = Value::Null;
        assert_eq!(cache_bound_held(&cache), None);
        let mut row = json!({"status":"ok","elapsed_ns":100,"workload_value":3 * shared::GB,
            "measurement_scope":"prepared_operation",
            "measurement_contract":{"text_backing":"store","production_path":true,"code_path":"Chunk::load","fixture_kind":"non_repeating","fixture_revision":2},
            "workload_details":{"database_bytes":3 * shared::GB,"history_revisions":128},
            "phase_elapsed_ns":{"cold_read_p99_ns":3000000,"physical_read_verified_count":0,"concurrent_saves":3}});
        assert!(!proven("cold_reads_during_save", &row));
        row["phase_elapsed_ns"]["physical_read_verified_count"] = json!(128);
        assert!(!proven("cold_reads_during_save", &row));
        row["phase_elapsed_ns"]["physical_io_scope"] = json!("foreground_reader_thread_read_bytes");
        row["phase_elapsed_ns"]["read_sample_count"] = json!(128);
        row["phase_elapsed_ns"]["cold_read_samples"] = json!(vec![
            json!({
                "elapsed_ns":3000000,"eviction_succeeded":true,
                "physical_read_verified":true,"read_bytes":4096
            });
            128
        ]);
        assert!(proven("cold_reads_during_save", &row));
        for (field, value) in [
            (
                "physical_io_scope",
                json!("process read_bytes during each read; includes concurrent saves"),
            ),
            ("physical_read_verified_count", json!(127)),
            ("read_sample_count", json!(127)),
            ("cold_read_p99_ns", json!(1)),
        ] {
            let mut invalid = row.clone();
            invalid["phase_elapsed_ns"][field] = value;
            assert!(!proven("cold_reads_during_save", &invalid), "{field}");
        }
        let mut cached = row.clone();
        cached["phase_elapsed_ns"]["cold_read_samples"][0]["read_bytes"] = json!(0);
        assert!(!proven("cold_reads_during_save", &cached));
        row["phase_elapsed_ns"]["concurrent_saves"] = json!(0);
        assert!(!proven("cold_reads_during_save", &row));
    }

    #[test]
    fn project_scenarios_reject_missing_duplicate_and_invalid_declarations() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join("benches"))?;
        for (scenarios, valid) in [
            (json!([]), false),
            (json!(["renamed", "renamed"]), false),
            (json!([null]), false),
            (json!(["large_file_first_visible_paint"]), true),
        ] {
            std::fs::write(
                root.path().join("benches/measurement_manifest.json"),
                json!({"probes":{"resource_probe":scenarios}}).to_string(),
            )?;
            assert_eq!(expected_scenarios(root.path()).is_ok(), valid);
        }
        Ok(())
    }

    #[test]
    fn resource_evidence_requires_live_retention_metrics_and_current_history_contracts() {
        let mut retained = json!({"workload_value":100000,"status":"ok","elapsed_ns":10,
            "measurement_scope":"scenario_including_fixture_work","baseline_live_bytes":100,"live_bytes":100,"live_growth_bytes":0});
        assert!(!proven("provenance_retained_memory", &retained));
        retained["retained_live_bytes_before_teardown"] = json!(1000);
        assert!(proven("provenance_retained_memory", &retained));
        let mut save = json!({"workload_value":50 * shared::MB,"status":"ok","elapsed_ns":10,
            "measurement_scope":"prepared_operation","phase_elapsed_ns":{"save_ns":9},
            "fixture_contract":{"equals":{"fixture_revision":2,"history_retention":"unbounded","inserted_bytes":1,"synchronous":"FULL"},"minimum":{"history_payload_bytes":50 * shared::MB,"history_entries":1025}},
            "workload_details":{"fixture_revision":2,"history_retention":"unbounded","history_payload_bytes":50 * shared::MB,"history_entries":1025,"inserted_bytes":1,"synchronous":"FULL"}});
        assert!(!proven("large_dirty_buffer_save", &save));
        save["process_resources"] = json!({"write_bytes":4096});
        assert!(proven("large_dirty_buffer_save", &save));
        for (key, value) in [
            ("history_entries", json!(1)),
            ("history_payload_bytes", json!(16 * shared::MB)),
            ("fixture_revision", json!(1)),
            ("history_retention", json!("bounded")),
        ] {
            let prior = std::mem::replace(&mut save["workload_details"][key], value);
            assert!(!proven("large_dirty_buffer_save", &save), "{key}");
            save["workload_details"][key] = prior;
        }
        let mut frame = json!({"workload_value":10000,"status":"ok","elapsed_ns":10,
            "measurement_scope":"prepared_operation","allocated_bytes":20,"peak_live_bytes":30});
        assert!(proven("tab_strip_frame_rendering", &frame));
        assert!(!proven("session_persist_cost", &frame));
        assert_eq!(
            gap_evidence(
                "tab_strip_frame_rendering",
                std::slice::from_ref(&frame),
                true
            )["closed"],
            false
        );
        frame["status"] = json!("panic");
        assert!(!proven("tab_strip_frame_rendering", &frame));
    }
}
