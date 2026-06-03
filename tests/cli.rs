use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_splens"))
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run_lens(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .arg("--config")
        .arg(repo_root().join("examples").join("scratchpad.toml"))
        .current_dir(repo_root())
        .output()
        .expect("splens should run")
}

#[test]
fn catalog_lists_performance_and_telemetry_tasks() {
    let output = run_lens(&["catalog"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    let task_ids: Vec<_> = payload["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|task| task["id"].as_str())
        .collect();
    assert!(task_ids.contains(&"performance.search"));
    assert!(task_ids.contains(&"performance.performance-review"));
    assert!(task_ids.contains(&"telemetry.app-package"));
}

#[test]
fn telemetry_prints_json() {
    let output = run_lens(&["telemetry"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(payload.get("manifest").is_some());
}

#[test]
fn project_code_metrics_runs_against_scratchpad_when_available() {
    let scratchpad = repo_root().parent().unwrap().join("scratchpad");
    if !scratchpad.exists() {
        return;
    }
    let output = run_lens(&["measure", "project-code"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let artifact = scratchpad
        .join("target")
        .join("analysis")
        .join("project_code_metrics.json");
    assert!(artifact.exists());
    let payload: Value = serde_json::from_str(&std::fs::read_to_string(artifact).unwrap()).unwrap();
    assert!(payload.get("current").is_some());
    assert!(payload.get("history").is_some());
}

#[test]
fn schema_export_writes_performance_review_contract() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(bin())
        .args(["schema", "export", "--output"])
        .arg(dir.path())
        .current_dir(repo_root())
        .output()
        .expect("splens schema export should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let index_path = dir.path().join("index.json");
    let schema_path = dir.path().join("performance_review.schema.json");
    assert!(index_path.exists());
    assert!(schema_path.exists());

    let index: Value = serde_json::from_str(&std::fs::read_to_string(index_path).unwrap()).unwrap();
    assert_eq!(index["schemas"][0]["id"], "performance_review");

    let schema_text = std::fs::read_to_string(schema_path).unwrap();
    assert!(schema_text.contains("promise_health"));
    assert!(schema_text.contains("failed_before_promise"));
}
