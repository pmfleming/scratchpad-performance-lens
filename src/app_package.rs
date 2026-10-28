use anyhow::Result;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

const SESSION_DIR_NAME: &str = "scratchpad";
const SESSION_MANIFEST_NAME: &str = "session.json";
const SESSION_ERROR_LOG_NAME: &str = "error.log";
const SESSION_BUFFER_EXTENSION: &str = ".tmp";

pub fn payload() -> Result<Value> {
    payload_at(&app_session_root())
}

fn payload_at(root: &Path) -> Result<Value> {
    let manifest_path = root.join(SESSION_MANIFEST_NAME);
    let error_log_path = root.join(SESSION_ERROR_LOG_NAME);
    let mut warnings = Vec::new();
    let manifest = load_json_file(&manifest_path, &mut warnings);
    let mut diagnostic_count = 0;
    let diagnostics = load_ndjson_file(&error_log_path, &mut warnings, &mut diagnostic_count);
    let buffers = flatten_session_buffers(manifest.as_ref(), root);
    let buffer_files = if root.exists() {
        std::fs::read_dir(root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "tmp"))
            .map(|path| file_info(&path))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let tabs = manifest.as_ref().and_then(|value| value["tabs"].as_array());
    let view_count: usize = iter_session_tabs(manifest.as_ref())
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
            "version": manifest.as_ref().and_then(|value| value.get("version")),
            "active_tab_index": manifest.as_ref().and_then(|value| value.get("active_tab_index")),
            "tab_count": tabs.map_or(0, Vec::len),
            "buffer_count": buffers.len(),
            "view_count": view_count,
            "dirty_buffer_count": buffers.iter().filter(|buffer| buffer.get("is_dirty").and_then(Value::as_bool).unwrap_or(false)).count(),
            "snapshot_file_count": buffer_files.len(),
            "missing_snapshot_count": buffers.iter().filter(|buffer| !buffer.get("snapshot").and_then(|snapshot| snapshot.get("exists")).and_then(Value::as_bool).unwrap_or(false)).count(),
            "diagnostic_count": diagnostic_count,
        },
        "buffers": buffers,
        "buffer_files": buffer_files,
        "topology": session_topology_summary(manifest.as_ref()),
        "diagnostics": diagnostics,
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

fn load_ndjson_file(
    path: &Path,
    warnings: &mut Vec<String>,
    total_count: &mut usize,
) -> Vec<Value> {
    const MAX_DIAGNOSTICS: usize = 500;

    if !path.exists() {
        return Vec::new();
    }
    let Ok(file) = std::fs::File::open(path) else {
        warnings.push(format!(
            "Could not read {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        return Vec::new();
    };
    let mut diagnostics = VecDeque::with_capacity(MAX_DIAGNOSTICS);
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let Ok(line) = line else {
            warnings.push(format!(
                "Could not read {} line {}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                index + 1
            ));
            break;
        };
        let location = format!(
            "{} line {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            index + 1
        );
        for mut value in diagnostic_objects(&line, &location, warnings) {
            value["line"] = json!(index + 1);
            *total_count += 1;
            if diagnostics.len() == MAX_DIAGNOSTICS {
                diagnostics.pop_front();
            }
            diagnostics.push_back(value);
        }
    }
    diagnostics.into_iter().collect()
}

fn diagnostic_objects<'a>(
    line: &'a str,
    location: &'a str,
    warnings: &'a mut Vec<String>,
) -> impl Iterator<Item = Value> + 'a {
    // The deserializer stops after a malformed fragment, but not after a non-object.
    serde_json::Deserializer::from_str(line)
        .into_iter::<Value>()
        .filter_map(move |parsed| match parsed {
            Ok(value) if value.is_object() => Some(value),
            Ok(_) => {
                warnings.push(format!("Skipped non-object {location}"));
                None
            }
            Err(error) => {
                warnings.push(format!("Skipped malformed {location}: {error}"));
                None
            }
        })
}

fn iter_session_tabs(manifest: Option<&Value>) -> impl Iterator<Item = (usize, &Value)> {
    manifest
        .and_then(|value| value["tabs"].as_array())
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, tab)| tab.is_object())
}

fn tab_buffer_entries(tab: &Value) -> Vec<Cow<'_, Value>> {
    let buffers = tab["buffers"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if !buffers.is_empty() || tab.get("temp_id").is_none() {
        return buffers
            .iter()
            .filter(|buffer| buffer.is_object())
            .map(Cow::Borrowed)
            .collect();
    }
    vec![Cow::Owned(json!({
        "id": tab.get("buffer_id"),
        "name": tab.get("name"),
        "path": tab.get("path"),
        "is_dirty": tab.get("is_dirty"),
        "temp_id": tab.get("temp_id"),
        "encoding": tab.get("encoding"),
        "has_bom": tab.get("has_bom"),
    }))]
}

fn flatten_session_buffers(manifest: Option<&Value>, root: &Path) -> Vec<Value> {
    let mut buffers = Vec::new();
    for (tab_index, tab) in iter_session_tabs(manifest) {
        for (buffer_index, buffer) in tab_buffer_entries(tab).into_iter().enumerate() {
            buffers.push(session_buffer_row(
                tab,
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
        "id": buffer.get("id"),
        "name": buffer.get("name").or_else(|| tab.get("name")).unwrap_or(&json!("Untitled")),
        "path": buffer.get("path").or_else(|| tab.get("path")),
        "is_dirty": buffer.get("is_dirty").and_then(Value::as_bool).unwrap_or(false),
        "is_settings_file": buffer.get("is_settings_file").and_then(Value::as_bool).unwrap_or(false),
        "temp_id": temp_id,
        "encoding": buffer.get("encoding").or_else(|| buffer.get("format").and_then(|format| format.get("encoding_name"))),
        "has_bom": buffer.get("has_bom").or_else(|| buffer.get("format").and_then(|format| format.get("has_bom"))),
        "disk_modified_millis": buffer.get("disk_modified_millis"),
        "disk_len": buffer.get("disk_len"),
        "text_history_count": buffer.get("text_history").and_then(Value::as_array).map_or(0, Vec::len),
        "snapshot": snapshot_path.as_ref().map_or_else(|| json!({"exists": false}), |path| file_info(path)),
    })
}

fn session_topology_summary(manifest: Option<&Value>) -> Vec<Value> {
    iter_session_tabs(manifest)
        .map(|(tab_index, tab)| {
            let views = tab["views"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            json!({
                "tab_index": tab_index,
                "active_view_id": tab.get("active_view_id"),
                "view_count": views.len(),
                "view_ids": views.iter().filter_map(|view| view.get("id")).collect::<Vec<_>>(),
                "root_pane_kind": root_pane_kind(tab.get("root_pane")),
            })
        })
        .collect()
}

fn root_pane_kind(node: Option<&Value>) -> &str {
    let Some(Value::Object(map)) = node else {
        return "unknown";
    };
    if map.contains_key("Leaf") || map.contains_key("leaf") || map.contains_key("view_id") {
        "leaf"
    } else if map.contains_key("Split") || map.contains_key("split") || map.contains_key("axis") {
        "split"
    } else {
        map.keys().next().map(String::as_str).unwrap_or("unknown")
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
    use super::{load_ndjson_file, payload_at, root_pane_kind};
    use serde_json::json;

    #[test]
    fn diagnostics_stream_objects_recover_at_newlines_and_bound_retention() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("error.log");
        let mut warnings = Vec::new();
        let mut total = 0;
        assert!(load_ndjson_file(&path, &mut warnings, &mut total).is_empty());
        std::fs::write(
            &path,
            "{\"kind\":\"first\"}{\"kind\":\"second\"} null {} trailing\n\n{}",
        )?;
        let rows = load_ndjson_file(&path, &mut warnings, &mut total);
        assert_eq!(total, 4);
        assert_eq!(
            rows.iter().map(|row| &row["line"]).collect::<Vec<_>>(),
            [&json!(1), &json!(1), &json!(1), &json!(3)]
        );
        assert_eq!(rows[0]["kind"], "first");
        assert_eq!(rows[1]["kind"], "second");
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0], "Skipped non-object error.log line 1");
        assert!(warnings[1].starts_with("Skipped malformed error.log line 1:"));
        std::fs::write(&path, "{}\n".repeat(501))?;
        total = 0;
        let rows = load_ndjson_file(&path, &mut warnings, &mut total);
        assert_eq!((total, rows.len()), (501, 500));
        assert_eq!(rows[0]["line"], 2);
        assert_eq!(rows[499]["line"], 501);
        Ok(())
    }

    #[test]
    fn telemetry_handles_missing_corrupt_legacy_and_modern_sessions() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(payload_at(&dir.path().join("missing"))?["exists"], false);
        let manifest = dir.path().join("session.json");
        std::fs::write(&manifest, "{")?;
        assert_eq!(
            payload_at(dir.path())?["warnings"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        std::fs::write(dir.path().join("saved.tmp"), "snapshot")?;
        std::fs::write(&manifest, json!({"tabs":[null,
            {"buffer_id":7,"temp_id":"saved","name":"Legacy","is_dirty":true},
            {"name":"Modern","buffers":[false,{"temp_id":"missing","format":{"encoding_name":"UTF-8","has_bom":true},"text_history":[{}]}],"views":[{"id":9},{}],"root_pane":{"Split":{}}},
            {"buffers":[{}]}, {"buffers":[]}
        ]}).to_string())?;
        let report = payload_at(dir.path())?;
        let summary = &report["manifest_summary"];
        for (key, expected) in [
            ("tab_count", 5),
            ("buffer_count", 3),
            ("view_count", 2),
            ("dirty_buffer_count", 1),
            ("snapshot_file_count", 1),
            ("missing_snapshot_count", 2),
        ] {
            assert_eq!(summary[key], expected, "{key}");
        }
        let buffers = &report["buffers"];
        assert_eq!(buffers[0]["id"], 7);
        assert_eq!(buffers[0]["tab_index"], 1);
        assert_eq!(buffers[0]["snapshot"]["size_bytes"], 8);
        assert_eq!(buffers[1]["encoding"], "UTF-8");
        assert_eq!(buffers[1]["has_bom"], true);
        assert_eq!(buffers[1]["text_history_count"], 1);
        assert_eq!(buffers[2]["name"], "Untitled");
        assert_eq!(report["topology"][1]["view_ids"], json!([9]));
        for (node, kind) in [
            (json!({"Leaf":9}), "leaf"),
            (json!({"axis":"x"}), "split"),
            (json!({"Custom":{}}), "Custom"),
            (json!({}), "unknown"),
            (json!(null), "unknown"),
        ] {
            assert_eq!(root_pane_kind(Some(&node)), kind);
        }
        Ok(())
    }
}
