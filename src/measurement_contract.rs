//! Code-path requirements for evidence that makes storage-sensitive claims.
use serde_json::Value;

pub fn requires_store(id: &str) -> bool {
    matches!(
        id,
        "large_file_background_index_ceiling"
            | "search_file_size_ceiling"
            | "paste_size_ceiling"
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
    contract["production_path"] == true
        && contract["text_backing"] == "store"
        && contract["code_path"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        && (!id.starts_with("editor_scroll_")
            || contract["document_bytes"]
                .as_u64()
                .is_some_and(|bytes| bytes > 16 * 1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn size_labels_and_owned_component_paths_cannot_prove_store_promises() {
        let mut contract =
            json!({"production_path":true,"text_backing":"owned","code_path":"search"});
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
