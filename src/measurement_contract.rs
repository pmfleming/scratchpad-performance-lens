//! Code-path requirements for evidence that makes storage-sensitive claims.
use serde_json::Value;

pub fn requires_store(id: &str) -> bool {
    matches!(
        id,
        "large_file_background_index_ceiling"
            | "search_file_size_ceiling"
            | "paste_size_ceiling"
            | "production_paste"
            | "cold_reads_during_save"
            | "aged_store_save"
            | "aged_store_restore"
            | "aged_store_close"
            | "aged_store_forget"
            | "store_import"
            | "ui_during_store_writes"
            | "large_utf8_load_peak_memory"
            | "file_backed_search"
            | "file_backed_random_window"
            | "file_backed_chunk_cache_tracking"
    ) || id.starts_with("editor_scroll_")
}

pub fn eligible(id: &str, contract: &Value) -> bool {
    if !requires_store(id) {
        return true;
    }
    contract["fixture_kind"] == "non_repeating"
        && contract["fixture_revision"].as_u64().is_some_and(|r| r > 0)
        && contract["production_path"] == true
        && contract["text_backing"] == "store"
        && contract["code_path"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        && (id != "search_file_size_ceiling"
            || (contract["code_path"] == "search_target_ranges/production_worker_policy"
                && contract["search_fixture_revision"] == 3
                && contract["completion_endpoint"] == "all_ranges_returned"
                && contract["expected_matches"] == 1
                && contract["document_bytes"].as_u64().is_some_and(|n| n > 0)))
        && (!id.starts_with("editor_scroll_")
            || contract["document_bytes"]
                .as_u64()
                .is_some_and(|bytes| bytes > 16 * 1024 * 1024))
}

pub fn completed_search(
    id: &str,
    workload: Option<i64>,
    result: Option<u64>,
    scope: Option<&str>,
    contract: &Value,
) -> bool {
    id != "search_file_size_ceiling"
        || (eligible(id, contract)
            && scope == Some("prepared_search_component_completion")
            && workload.is_some_and(|n| n > 0 && Some(n) == contract["document_bytes"].as_i64())
            && result == Some(1))
}

/// A fresh import must materialize most of its logical body in the store.
/// This is an integrity check, not a write-volume budget.
pub fn import_fixture_valid(event: &Value) -> bool {
    let phases = &event["phase_elapsed_ns"];
    match (
        phases["import_logical_bytes"].as_u64(),
        phases["import_database_growth_bytes"].as_u64(),
    ) {
        (Some(logical), Some(stored)) => logical > 0 && stored >= logical / 2,
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::eligible;
    use serde_json::json;
    #[test]
    fn production_search_requires_exact_input_and_complete_matching() {
        let contract = json!({"fixture_kind":"non_repeating","fixture_revision":2,"production_path":true,"text_backing":"store",
            "code_path":"search_target_ranges/production_worker_policy","search_fixture_revision":3,
            "document_bytes":1048576,"expected_matches":1,"completion_endpoint":"all_ranges_returned"});
        let valid = |bytes, count, contract: &serde_json::Value| {
            super::completed_search(
                "search_file_size_ceiling",
                Some(bytes),
                count,
                Some("prepared_search_component_completion"),
                contract,
            )
        };
        assert!(valid(1048576, Some(1), &contract));
        assert!(!valid(1073741824, Some(1), &contract));
        assert!(!valid(1048576, None, &contract));
        assert!(!valid(1048576, Some(0), &contract));
        for (key, value) in [
            (
                "code_path",
                json!("DocumentSnapshot::chunks_for_range/search_program"),
            ),
            ("search_fixture_revision", json!(2)),
            ("completion_endpoint", json!("partial_ranges")),
            ("expected_matches", json!(0)),
        ] {
            let mut invalid = contract.clone();
            invalid[key] = value;
            assert!(!valid(1048576, Some(1), &invalid));
        }
    }
    #[test]
    fn dedup_fixtures_and_disproportionately_small_imports_are_not_proof() {
        assert!(!eligible(
            "file_backed_search",
            &json!({"fixture_kind":"dedup","fixture_revision":2,
            "production_path":true,"text_backing":"store","code_path":"search"})
        ));
        assert!(!super::import_fixture_valid(&json!({"phase_elapsed_ns":{
            "import_logical_bytes":1073741824u64,"import_database_growth_bytes":20 * 1024 * 1024}})));
        assert!(super::import_fixture_valid(&json!({"phase_elapsed_ns":{
            "import_logical_bytes":1073741824u64,"import_database_growth_bytes":1080000000u64}})));
    }
    #[test]
    fn size_labels_and_owned_component_paths_cannot_prove_store_promises() {
        let mut contract = json!({"fixture_kind":"non_repeating","fixture_revision":2,"production_path":true,"text_backing":"owned",
            "code_path":"search_target_ranges/production_worker_policy","search_fixture_revision":3,
            "document_bytes":1048576,"expected_matches":1,"completion_endpoint":"all_ranges_returned"});
        assert!(!eligible("search_file_size_ceiling", &contract));
        contract["text_backing"] = json!("store");
        assert!(eligible("search_file_size_ceiling", &contract));
        assert!(!eligible("editor_scroll_frame_120hz", &contract));
        contract["document_bytes"] = json!(64 * 1024 * 1024);
        assert!(eligible("editor_scroll_frame_120hz", &contract));
        contract["production_path"] = json!(false);
        assert!(!eligible("search_file_size_ceiling", &contract));
    }
}
