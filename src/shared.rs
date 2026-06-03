use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use walkdir::WalkDir;

pub const MB: i64 = 1024 * 1024;
pub const GB: i64 = 1024 * MB;

#[derive(Clone, Debug)]
pub struct FlamegraphConfig {
    pub id: &'static str,
    pub name: &'static str,
    pub cargo_args: &'static [&'static str],
    pub benchmark_keys: &'static [&'static str],
    pub workload_families: &'static [&'static str],
    pub coverage_role: &'static str,
    pub resource_focus: &'static str,
    pub description: &'static str,
}

const BUILTIN_METADATA_JSON: &str = include_str!("../metadata/builtin_benchmark_metadata.json");

pub const FLAMEGRAPH_CONFIGS: &[FlamegraphConfig] = &[
    FlamegraphConfig { id: "tab_operations_profile", name: "Tab Operations Profile", cargo_args: &["--bin", "profile_tab_operations"], benchmark_keys: &["tab_stress_operations", "tab_count_scale"], workload_families: &["tab-management"], coverage_role: "report-driven", resource_focus: "cpu", description: "Tab activation, reorder, and multi-tab movement hot path." },
    FlamegraphConfig { id: "tab_tile_layout_profile", name: "Tab Tile Layout Profile", cargo_args: &["--bin", "profile_tab_tile_layout"], benchmark_keys: &["tile_count_scale"], workload_families: &["split-layout"], coverage_role: "report-driven", resource_focus: "cpu", description: "Split resize, rebalance, and tile layout hot path." },
    FlamegraphConfig { id: "view_navigation_profile", name: "View Navigation Profile", cargo_args: &["--bin", "profile_view_navigation"], benchmark_keys: &[], workload_families: &["exploratory"], coverage_role: "exploratory", resource_focus: "cpu", description: "Exploratory editor-view navigation profile without a dedicated broad benchmark family." },
    FlamegraphConfig { id: "search_current_app_state_profile", name: "Search Current App-State Profile", cargo_args: &["--bin", "profile_search_current_app_state"], benchmark_keys: &["search_active_completion_file_size", "search_active_first_response_file_size", "search_current_completion_file_size", "search_current_completion_aggregate_size", "search_current_app_state_completion_aggregate_size"], workload_families: &["search"], coverage_role: "report-driven", resource_focus: "cpu", description: "Active-file and current-tab search hot path through the Scratchpad search pipeline." },
    FlamegraphConfig { id: "search_all_tabs_profile", name: "Search All Tabs Profile", cargo_args: &["--bin", "profile_search_all_tabs"], benchmark_keys: &["search_all_completion_file_size", "search_all_completion_aggregate_size"], workload_families: &["search"], coverage_role: "report-driven", resource_focus: "cpu", description: "All-open-tabs search hot path across the global workspace tab manager." },
    FlamegraphConfig { id: "search_dispatch_profile", name: "Search Dispatch Profile", cargo_args: &["--bin", "profile_search_dispatch"], benchmark_keys: &["search_current_dispatch_aggregate_size", "search_all_dispatch_aggregate_size"], workload_families: &["search-dispatch"], coverage_role: "report-driven", resource_focus: "cpu", description: "Search request building and target collection cost before worker-side scanning begins." },
    FlamegraphConfig { id: "document_snapshot_profile", name: "Document Snapshot Profile", cargo_args: &["--bin", "profile_document_snapshot"], benchmark_keys: &["document_snapshot_creation_latency"], workload_families: &["snapshot"], coverage_role: "report-driven", resource_focus: "cpu", description: "Revisioned snapshot creation cost for large piece-tree-backed documents." },
    FlamegraphConfig { id: "viewport_extraction_profile", name: "Viewport Extraction Profile", cargo_args: &["--bin", "profile_viewport_extraction"], benchmark_keys: &["viewport_extraction_latency"], workload_families: &["viewport"], coverage_role: "report-driven", resource_focus: "cpu", description: "Piece-tree visible-range and overscanned viewport extraction cost for expanded buffers." },
    FlamegraphConfig { id: "ui_render_frame_profile", name: "UI Render Frame Profile", cargo_args: &["--bin", "profile_ui_render_frame"], benchmark_keys: &["ui_render_frame_120hz", "editor_scroll_frame_120hz", "ui_render_frame"], workload_families: &["scroll"], coverage_role: "report-driven", resource_focus: "cpu", description: "Headless Scratchpad frame loop profile with phase timing instrumentation." },
    FlamegraphConfig { id: "scroll_stress_profile", name: "Scroll Stress Profile", cargo_args: &["--bin", "profile_scroll_stress"], benchmark_keys: &["scroll_stress_latency"], workload_families: &["scroll"], coverage_role: "report-driven", resource_focus: "cpu", description: "Headless editor layout and repaint work representative of repeated scroll redraw." },
    FlamegraphConfig { id: "paste_stress_profile", name: "Paste Stress Profile", cargo_args: &["--bin", "profile_paste_stress"], benchmark_keys: &["paste_stress_latency"], workload_families: &["edit-paste"], coverage_role: "report-driven", resource_focus: "cpu", description: "Large insert into an expanded buffer, including metadata refresh and undo state updates." },
    FlamegraphConfig { id: "split_stress_profile", name: "Split Stress Profile", cargo_args: &["--bin", "profile_split_stress"], benchmark_keys: &["split_stress_latency"], workload_families: &["split-layout"], coverage_role: "report-driven", resource_focus: "cpu", description: "Repeated splitting and rebalance work on expanded document tiles." },
    FlamegraphConfig { id: "search_capacity_profile", name: "Search Capacity Profile", cargo_args: &["--bin", "profile_search_capacity"], benchmark_keys: &["search_capacity"], workload_families: &["search"], coverage_role: "exploratory", resource_focus: "cpu", description: "Standalone large-text search profile for the upper end of file-size scaling." },
];

fn builtin_benchmark_metadata() -> Vec<Value> {
    serde_json::from_str(BUILTIN_METADATA_JSON)
        .expect("built-in benchmark metadata JSON should be valid")
}

pub fn load_benchmark_metadata(project_root: &Path) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();
    for item in builtin_benchmark_metadata() {
        let Value::Object(mut object) = item else {
            continue;
        };
        let Some(Value::String(key)) = object.remove("key") else {
            continue;
        };
        object.insert("threshold_authoritative".to_string(), json!(false));
        object.insert("threshold_source".to_string(), json!("builtin_fallback"));
        object.insert("metadata_source".to_string(), json!("builtin_fallback"));
        object.insert("stale_budget_risk".to_string(), json!(true));
        object
            .entry("targets".to_string())
            .or_insert_with(|| json!([]));
        object
            .entry("limiting_resource_hint".to_string())
            .or_insert_with(|| json!("cpu"));
        metadata.insert(key, Value::Object(object));
    }
    for relative in [
        "benches/benchmark_targets.json",
        "benches/search_benchmark_targets.json",
    ] {
        let path = project_root.join(relative);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(Value::Object(entries)) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let is_search_metadata = path
            .file_name()
            .is_some_and(|name| name == "search_benchmark_targets.json");
        for (key, value) in entries {
            if let Value::Object(mut object) = value {
                let family = object
                    .remove("family")
                    .or_else(|| object.get("workload_family").cloned())
                    .unwrap_or_else(|| {
                        json!(if is_search_metadata {
                            "search"
                        } else {
                            "unmapped"
                        })
                    });
                object.insert("workload_family".to_string(), family);
                object.entry("kind".to_string()).or_insert_with(|| {
                    json!(if is_search_metadata {
                        "workflow"
                    } else {
                        "unmapped"
                    })
                });
                let threshold_authoritative =
                    object.get("threshold_ms").and_then(Value::as_f64).is_some();
                object.insert(
                    "threshold_authoritative".to_string(),
                    json!(threshold_authoritative),
                );
                object.insert(
                    "threshold_source".to_string(),
                    json!(if threshold_authoritative {
                        relative.replace('\\', "/")
                    } else {
                        "missing".to_string()
                    }),
                );
                object.insert(
                    "metadata_source".to_string(),
                    json!(relative.replace('\\', "/")),
                );
                object.insert(
                    "stale_budget_risk".to_string(),
                    json!(!threshold_authoritative),
                );
                object
                    .entry("targets".to_string())
                    .or_insert_with(|| json!([]));
                object
                    .entry("limiting_resource_hint".to_string())
                    .or_insert_with(|| json!("cpu"));
                metadata.insert(key, Value::Object(object));
            }
        }
    }

    metadata
}

pub fn authoritative_threshold_ms(meta: &Value) -> Option<f64> {
    meta.get("threshold_authoritative")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        .then(|| meta.get("threshold_ms").and_then(Value::as_f64))
        .flatten()
}

pub fn matching_flamegraph_ids(benchmark_key: &str) -> Vec<String> {
    FLAMEGRAPH_CONFIGS
        .iter()
        .filter(|config| config.benchmark_keys.contains(&benchmark_key))
        .map(|config| config.id.to_string())
        .collect()
}

pub fn flamegraph_json(config: &FlamegraphConfig, output_dir: &Path, issue: Option<&str>) -> Value {
    let svg_path = output_dir.join(format!("{}.svg", config.id));
    let available = svg_path.exists();
    let mut item = json!({
        "id": config.id,
        "name": config.name,
        "path": format!("flamegraphs/{}.svg", config.id),
        "type": if available { "svg" } else { "missing" },
        "available": available,
        "benchmark_keys": config.benchmark_keys,
        "workload_families": config.workload_families,
        "coverage_role": config.coverage_role,
        "resource_focus": config.resource_focus,
        "description": config.description,
    });
    if let Some(issue) = issue {
        if !available {
            item["issue"] = json!(issue);
        }
    }
    item
}

pub fn run_command(project_root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let (program, rest) = args.split_first().context("empty command")?;
    Command::new(program)
        .args(rest)
        .current_dir(project_root)
        .output()
        .with_context(|| format!("failed to run {}", args.join(" ")))
}

pub fn run_progress_command(project_root: &Path, args: &[&str], label: &str) -> Result<()> {
    eprintln!("{label}");
    let status = Command::new(args[0])
        .args(&args[1..])
        .current_dir(project_root)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("failed to run {}", args.join(" ")))?;
    if !status.success() {
        anyhow::bail!("{} exited with {}", args.join(" "), status);
    }
    Ok(())
}

pub fn criterion_estimate_paths(criterion_dir: &Path) -> Vec<PathBuf> {
    if !criterion_dir.exists() {
        return Vec::new();
    }
    WalkDir::new(criterion_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file() && entry.file_name() == "estimates.json")
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.parent()
                .is_some_and(|parent| parent.file_name().is_some_and(|name| name == "new"))
        })
        .collect()
}

pub fn benchmark_name_from_estimate_path(
    criterion_dir: &Path,
    estimates_path: &Path,
) -> Option<String> {
    let relative = estimates_path.strip_prefix(criterion_dir).ok()?;
    let parts: Vec<_> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().to_string())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    Some(parts[..parts.len() - 2].join("/"))
}

pub fn benchmark_key(name: &str) -> &str {
    name.split_once('/').map_or(name, |(head, _)| head)
}

pub fn human_bytes(value: Option<i64>) -> String {
    let Some(value) = value else {
        return "-".to_string();
    };
    if value >= GB {
        format!("{:.1} GB", value as f64 / GB as f64)
    } else if value >= MB {
        format!("{:.1} MB", value as f64 / MB as f64)
    } else if value >= 1024 {
        format!("{:.0} KB", value as f64 / 1024.0)
    } else {
        format!("{value} B")
    }
}

pub fn workload_label(value: i64, unit: &str) -> String {
    if unit == "bytes" {
        human_bytes(Some(value))
    } else {
        format!("{value} {unit}")
    }
}

pub fn safe_delta(last: Option<i64>, first: Option<i64>) -> Option<i64> {
    Some(last? - first?)
}

pub fn read_json(path: &Path, default: Value) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(default)
}

pub fn write_visibility(
    output_path: &Path,
    payload: &Value,
    label: &str,
    cli_text: String,
) -> Result<()> {
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        output_path,
        format!("{}\n", serde_json::to_string_pretty(payload)?),
    )?;
    println!("Wrote {label} visibility data to {}", output_path.display());
    println!("{cli_text}");
    Ok(())
}

pub fn f64_field(value: &Value, path: &[&str]) -> f64 {
    let mut current = value;
    for key in path {
        current = current.get(*key).unwrap_or(&Value::Null);
    }
    current.as_f64().unwrap_or(0.0)
}

pub fn string_field<'a>(value: &'a Value, key: &str, default: &'a str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_string()
}

pub fn array_strings(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{authoritative_threshold_ms, load_benchmark_metadata};
    use serde_json::json;

    #[test]
    fn builtin_metadata_thresholds_are_not_authoritative() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = load_benchmark_metadata(dir.path());
        let file_load = metadata.get("file_load").unwrap();
        assert_eq!(file_load["metadata_source"], json!("builtin_fallback"));
        assert_eq!(file_load["threshold_authoritative"], json!(false));
        assert_eq!(file_load["stale_budget_risk"], json!(true));
        assert_eq!(authoritative_threshold_ms(file_load), None);
    }

    #[test]
    fn repository_metadata_thresholds_are_authoritative() {
        let dir = tempfile::tempdir().unwrap();
        let benches = dir.path().join("benches");
        std::fs::create_dir_all(&benches).unwrap();
        std::fs::write(
            benches.join("search_benchmark_targets.json"),
            r#"{
                "search_current_files_completion": {
                    "threshold_ms": 80.0,
                    "workload_family": "search",
                    "targets": ["src/app/services/search.rs"]
                }
            }"#,
        )
        .unwrap();

        let metadata = load_benchmark_metadata(dir.path());
        let row = metadata.get("search_current_files_completion").unwrap();
        assert_eq!(
            row["metadata_source"],
            json!("benches/search_benchmark_targets.json")
        );
        assert_eq!(row["threshold_authoritative"], json!(true));
        assert_eq!(row["stale_budget_risk"], json!(false));
        assert_eq!(authoritative_threshold_ms(row), Some(80.0));
    }

    #[test]
    fn repository_metadata_without_threshold_is_not_budget_authoritative() {
        let dir = tempfile::tempdir().unwrap();
        let benches = dir.path().join("benches");
        std::fs::create_dir_all(&benches).unwrap();
        std::fs::write(
            benches.join("search_benchmark_targets.json"),
            r#"{
                "search_without_budget": {
                    "workload_family": "search",
                    "targets": ["src/app/services/search.rs"]
                }
            }"#,
        )
        .unwrap();

        let metadata = load_benchmark_metadata(dir.path());
        let row = metadata.get("search_without_budget").unwrap();
        assert_eq!(
            row["metadata_source"],
            json!("benches/search_benchmark_targets.json")
        );
        assert_eq!(row["threshold_source"], json!("missing"));
        assert_eq!(row["threshold_authoritative"], json!(false));
        assert_eq!(row["stale_budget_risk"], json!(true));
        assert_eq!(authoritative_threshold_ms(row), None);
    }
}
