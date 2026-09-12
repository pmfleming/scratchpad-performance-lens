use serde_json::{json, Value};
use std::process::Command;

#[test]
fn probes_use_cargo_executables_with_environment_or_configured_target_directories() {
    for via_environment in [true, false] {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("src/bin")).unwrap();
        std::fs::write(root.path().join("Cargo.toml"), "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n").unwrap();
        let report = json!({"meta":{},"scenarios":[{"scenario_id":"test", "frame_count":10,
            "p95_ms":1,"p99_ms":2,"budget_ms":8,"p99_budget_ms":12}]}).to_string();
        std::fs::write(root.path().join("src/bin/frame_metrics.rs"),
            format!("fn main() {{ print!(\"{{}}\", {report:?}); }}")).unwrap();
        let destination = root.path().join("relocated-build");
        std::fs::create_dir_all(root.path().join("target/release")).unwrap();
        std::fs::write(root.path().join("target/release/frame_metrics"), "must never execute stale binary").unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_splens"));
        command.args(["measure", "frame-metrics"]).current_dir(root.path()).env_remove("CARGO_TARGET_DIR");
        if via_environment { command.env("CARGO_TARGET_DIR", &destination); }
        else {
            std::fs::create_dir(root.path().join(".cargo")).unwrap();
            std::fs::write(root.path().join(".cargo/config.toml"), "[build]\ntarget-dir='relocated-build'\n").unwrap();
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let payload: Value = serde_json::from_slice(&std::fs::read(root.path().join("target/analysis/frame_metrics.json")).unwrap()).unwrap();
        assert!(std::path::Path::new(payload["meta"]["probe_executable"].as_str().unwrap()).starts_with(&destination));
    }
}

#[test]
fn corrupt_criterion_is_invalid_not_zero_and_fail_on_slow_is_nonzero() {
    let root = tempfile::tempdir().unwrap();
    let estimates = root.path().join("target/criterion/search_active_file_size/16384/new");
    std::fs::create_dir_all(&estimates).unwrap();
    std::fs::write(estimates.join("estimates.json"), "{truncated").unwrap();
    std::fs::create_dir(root.path().join("benches")).unwrap();
    std::fs::write(root.path().join("benches/search_benchmark_targets.json"), json!({
        "search_active_file_size": {"workload_family":"search", "threshold_ms":50}
    }).to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_splens"))
        .args(["measure", "search", "--skip-bench", "--fail-on-slow"])
        .env_remove("CARGO_TARGET_DIR").current_dir(root.path()).output().unwrap();
    assert!(!output.status.success());
    let rows: Value = serde_json::from_slice(&std::fs::read(root.path().join("target/analysis/search_speed.json")).unwrap()).unwrap();
    assert_eq!(rows[0]["measurement_status"], "invalid");
    assert!(rows[0]["mean_ns"].is_null());
    let provenance: Value = serde_json::from_slice(&std::fs::read(root.path().join("target/analysis/search_speed.json.provenance.json")).unwrap()).unwrap();
    assert_eq!(provenance["status"], "failed");
}
