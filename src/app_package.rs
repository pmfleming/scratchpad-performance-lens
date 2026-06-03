use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

const SESSION_DIR_NAME: &str = "scratchpad";
const SESSION_MANIFEST_NAME: &str = "session.json";
const SESSION_ERROR_LOG_NAME: &str = "error.log";
const SESSION_BUFFER_EXTENSION: &str = ".tmp";

pub fn payload() -> Result<Value> {
    let root = app_session_root();
    let manifest_path = root.join(SESSION_MANIFEST_NAME);
    let error_log_path = root.join(SESSION_ERROR_LOG_NAME);
    let mut warnings = Vec::new();
    let manifest = load_json_file(&manifest_path, &mut warnings);
    let diagnostics = load_ndjson_file(&error_log_path, &mut warnings);
    let buffers = flatten_session_buffers(manifest.as_ref(), &root);
    let buffer_files = if root.exists() {
        std::fs::read_dir(&root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "tmp"))
            .map(|path| file_info(&path))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let tabs = manifest
        .as_ref()
        .and_then(|value| value.get("tabs"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let view_count: usize = iter_session_tabs(manifest.as_ref())
        .iter()
        .map(|(_, tab)| {
            tab.get("views")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        })
        .sum();
    Ok(json!({
        "exists": root.exists(),
        "session_root": root.to_string_lossy(),
        "manifest_path": manifest_path.to_string_lossy(),
        "error_log_path": error_log_path.to_string_lossy(),
        "manifest_file": file_info(&manifest_path),
        "error_log_file": file_info(&error_log_path),
        "manifest": manifest,
        "manifest_summary": {
            "version": manifest.as_ref().and_then(|value| value.get("version")).cloned(),
            "active_tab_index": manifest.as_ref().and_then(|value| value.get("active_tab_index")).cloned(),
            "tab_count": tabs.len(),
            "buffer_count": buffers.len(),
            "view_count": view_count,
            "dirty_buffer_count": buffers.iter().filter(|buffer| buffer.get("is_dirty").and_then(Value::as_bool).unwrap_or(false)).count(),
            "snapshot_file_count": buffer_files.len(),
            "missing_snapshot_count": buffers.iter().filter(|buffer| !buffer.get("snapshot").and_then(|snapshot| snapshot.get("exists")).and_then(Value::as_bool).unwrap_or(false)).count(),
            "diagnostic_count": diagnostics.len(),
        },
        "buffers": buffers,
        "buffer_files": buffer_files,
        "topology": session_topology_summary(manifest.as_ref()),
        "diagnostics": diagnostics.into_iter().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>(),
        "warnings": warnings,
        "app_process_running": app_process_running(),
    }))
}

fn app_session_root() -> PathBuf {
    std::env::temp_dir().join(SESSION_DIR_NAME)
}

fn file_info(path: &Path) -> Value {
    match path.metadata() {
        Ok(metadata) => json!({
            "exists": true,
            "path": path.to_string_lossy(),
            "name": path.file_name().unwrap_or_default().to_string_lossy(),
            "size_bytes": metadata.len(),
            "modified_at": metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|duration| duration.as_secs_f64()),
        }),
        Err(error) => json!({
            "exists": false,
            "path": path.to_string_lossy(),
            "name": path.file_name().unwrap_or_default().to_string_lossy(),
            "error": error.to_string(),
        }),
    }
}

fn load_json_file(path: &Path, warnings: &mut Vec<String>) -> Option<Value> {
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(path).and_then(|text| {
        serde_json::from_str(&text)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }) {
        Ok(value) => Some(value),
        Err(error) => {
            warnings.push(format!(
                "Could not read {}: {error}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
            None
        }
    }
}

fn load_ndjson_file(path: &Path, warnings: &mut Vec<String>) -> Vec<Value> {
    if !path.exists() {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        warnings.push(format!(
            "Could not read {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let stream = serde_json::Deserializer::from_str(line).into_iter::<Value>();
        let mut parsed_any = false;
        for parsed in stream {
            match parsed {
                Ok(mut value) if value.is_object() => {
                    value["line"] = json!(index + 1);
                    diagnostics.push(value);
                    parsed_any = true;
                }
                Ok(_) => warnings.push(format!(
                    "Skipped non-object {} line {}",
                    path.file_name().unwrap_or_default().to_string_lossy(),
                    index + 1
                )),
                Err(error) => {
                    warnings.push(format!(
                        "Skipped malformed {} line {}: {error}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        index + 1
                    ));
                    break;
                }
            }
        }
        if !parsed_any && line.trim().is_empty() {
            continue;
        }
    }
    diagnostics
}

fn iter_session_tabs(manifest: Option<&Value>) -> Vec<(usize, Value)> {
    manifest
        .and_then(|value| value.get("tabs"))
        .and_then(Value::as_array)
        .map(|tabs| {
            tabs.iter()
                .enumerate()
                .filter(|(_, tab)| tab.is_object())
                .map(|(index, tab)| (index, tab.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn tab_buffer_entries(tab: &Value) -> Vec<Value> {
    let buffers = tab
        .get("buffers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !buffers.is_empty() || tab.get("temp_id").is_none() {
        return buffers.into_iter().filter(Value::is_object).collect();
    }
    vec![json!({
        "id": tab.get("buffer_id").cloned(),
        "name": tab.get("name").cloned(),
        "path": tab.get("path").cloned(),
        "is_dirty": tab.get("is_dirty").cloned(),
        "temp_id": tab.get("temp_id").cloned(),
        "encoding": tab.get("encoding").cloned(),
        "has_bom": tab.get("has_bom").cloned(),
    })]
}

fn flatten_session_buffers(manifest: Option<&Value>, root: &Path) -> Vec<Value> {
    let mut buffers = Vec::new();
    for (tab_index, tab) in iter_session_tabs(manifest) {
        for (buffer_index, buffer) in tab_buffer_entries(&tab).into_iter().enumerate() {
            buffers.push(session_buffer_row(
                &tab,
                &buffer,
                tab_index,
                buffer_index,
                root,
            ));
        }
    }
    buffers
}

fn session_buffer_row(
    tab: &Value,
    buffer: &Value,
    tab_index: usize,
    buffer_index: usize,
    root: &Path,
) -> Value {
    let temp_id = buffer.get("temp_id").and_then(Value::as_str);
    let snapshot_path = temp_id.map(|id| root.join(format!("{id}{SESSION_BUFFER_EXTENSION}")));
    json!({
        "tab_index": tab_index,
        "buffer_index": buffer_index,
        "id": buffer.get("id").cloned(),
        "name": buffer.get("name").or_else(|| tab.get("name")).cloned().unwrap_or_else(|| json!("Untitled")),
        "path": buffer.get("path").or_else(|| tab.get("path")).cloned(),
        "is_dirty": buffer.get("is_dirty").and_then(Value::as_bool).unwrap_or(false),
        "is_settings_file": buffer.get("is_settings_file").and_then(Value::as_bool).unwrap_or(false),
        "temp_id": temp_id,
        "encoding": buffer.get("encoding").or_else(|| buffer.get("format").and_then(|format| format.get("encoding_name"))).cloned(),
        "has_bom": buffer.get("has_bom").or_else(|| buffer.get("format").and_then(|format| format.get("has_bom"))).cloned(),
        "disk_modified_millis": buffer.get("disk_modified_millis").cloned(),
        "disk_len": buffer.get("disk_len").cloned(),
        "text_history_count": buffer.get("text_history").and_then(Value::as_array).map_or(0, Vec::len),
        "snapshot": snapshot_path.as_ref().map_or_else(|| json!({"exists": false}), |path| file_info(path)),
    })
}

fn session_topology_summary(manifest: Option<&Value>) -> Vec<Value> {
    iter_session_tabs(manifest)
        .into_iter()
        .map(|(tab_index, tab)| {
            let views = tab.get("views").and_then(Value::as_array).cloned().unwrap_or_default();
            json!({
                "tab_index": tab_index,
                "active_view_id": tab.get("active_view_id").cloned(),
                "view_count": views.len(),
                "view_ids": views.iter().filter_map(|view| view.get("id")).cloned().collect::<Vec<_>>(),
                "root_pane_kind": root_pane_kind(tab.get("root_pane")),
            })
        })
        .collect()
}

fn root_pane_kind(node: Option<&Value>) -> String {
    let Some(Value::Object(map)) = node else {
        return "unknown".to_string();
    };
    if map.contains_key("Leaf") || map.contains_key("leaf") || map.contains_key("view_id") {
        "leaf".to_string()
    } else if map.contains_key("Split") || map.contains_key("split") || map.contains_key("axis") {
        "split".to_string()
    } else {
        map.keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    }
}

fn app_process_running() -> bool {
    if cfg!(windows) {
        Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq scratchpad.exe", "/FO", "CSV", "/NH"])
            .output()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .to_lowercase()
                    .contains("scratchpad.exe")
            })
            .unwrap_or(false)
    } else {
        Command::new("pgrep")
            .args(["-x", "scratchpad"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::load_ndjson_file;

    #[test]
    fn load_ndjson_accepts_concatenated_objects_on_one_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("error.log");
        std::fs::write(&path, r#"{"kind":"first"}{"kind":"second"}"#).unwrap();
        let mut warnings = Vec::new();

        let diagnostics = load_ndjson_file(&path, &mut warnings);

        assert_eq!(warnings, Vec::<String>::new());
        assert_eq!(diagnostics[0]["kind"], "first");
        assert_eq!(diagnostics[1]["kind"], "second");
        assert_eq!(diagnostics[0]["line"], 1);
        assert_eq!(diagnostics[1]["line"], 1);
    }

    #[test]
    fn load_ndjson_keeps_valid_objects_before_malformed_fragment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("error.log");
        std::fs::write(&path, r#"{"kind":"valid"} trailing"#).unwrap();
        let mut warnings = Vec::new();

        let diagnostics = load_ndjson_file(&path, &mut warnings);

        assert_eq!(diagnostics[0]["kind"], "valid");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Skipped malformed error.log line 1"));
    }
}
