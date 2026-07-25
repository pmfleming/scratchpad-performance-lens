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
fn schema_export_writes_typed_artifact_contracts() {
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
    let capacity_schema_path = dir.path().join("capacity_report.schema.json");
    assert!(index_path.exists());
    assert!(schema_path.exists());
    assert!(capacity_schema_path.exists());

    let index: Value = serde_json::from_str(&std::fs::read_to_string(index_path).unwrap()).unwrap();
    assert_eq!(index["schemas"][0]["id"], "performance_review");
    assert!(index["schemas"]
        .as_array()
        .unwrap()
        .iter()
        .any(|schema| schema["id"] == "capacity_report"));

    let schema_text = std::fs::read_to_string(schema_path).unwrap();
    assert!(schema_text.contains("promise_health"));
    assert!(schema_text.contains("failed_before_promise"));
    let capacity_schema_text = std::fs::read_to_string(capacity_schema_path).unwrap();
    assert!(capacity_schema_text.contains("first_failure_workload"));
    assert!(capacity_schema_text.contains("workload_value"));
}

#[test]
fn all_continues_after_probe_failures_and_writes_review() {
    let dir = tempfile::tempdir().unwrap();
    let project_root = dir.path().join("empty-project");
    let output_dir = dir.path().join("analysis");
    std::fs::create_dir(&project_root).unwrap();
    let config_path = dir.path().join("splens.toml");
    std::fs::write(
        &config_path,
        format!(
            "project_name = \"fixture\"\nproject_root = {}\noutput_dir = {}\n",
            serde_json::to_string(&project_root.to_string_lossy()).unwrap(),
            serde_json::to_string(&output_dir.to_string_lossy()).unwrap(),
        ),
    )
    .unwrap();

    let output = Command::new(bin())
        .args(["measure", "all", "--skip-bench", "--config"])
        .arg(&config_path)
        .current_dir(repo_root())
        .output()
        .expect("splens measure all should run");

    assert!(!output.status.success());
    assert!(output_dir.join("performance_review.json").exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("measure producer(s) failed"), "{stderr}");
    assert!(stderr.contains("frame-metrics"), "{stderr}");
}

#[test]
fn rejects_flags_when_selected_tool_cannot_use_them() {
    let output = run_lens(&["measure", "capacity", "--fail-on-slow"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only applies"));
}
