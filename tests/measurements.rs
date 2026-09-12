use serde_json::{json, Value};
use std::process::Command;

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
