use serde_json::{json, Value};
use std::process::Command;

#[test]
fn probes_use_cargo_executables_with_environment_or_configured_target_directories(
) -> anyhow::Result<()> {
    for via_environment in [true, false] {
        let root = tempfile::tempdir()?;
        std::fs::create_dir_all(root.path().join("src/bin"))?;
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n",
        )?;
        let report = json!({"meta":{},"scenarios":[{"scenario_id":"test", "frame_count":10,
            "p95_ms":1,"p99_ms":2,"budget_ms":8,"p99_budget_ms":12}]})
        .to_string();
        std::fs::write(
            root.path().join("src/bin/frame_metrics.rs"),
            format!("fn main() {{ print!(\"{{}}\", {report:?}); }}"),
        )?;
        let destination = root.path().join("relocated-build");
        std::fs::create_dir_all(root.path().join("target/release"))?;
        std::fs::write(
            root.path().join("target/release/frame_metrics"),
            "must never execute stale binary",
        )?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_splens"));
        command
            .args(["measure", "frame-metrics"])
            .current_dir(root.path())
            .env_remove("CARGO_TARGET_DIR");
        if via_environment {
            command.env("CARGO_TARGET_DIR", &destination);
        } else {
            std::fs::create_dir(root.path().join(".cargo"))?;
            std::fs::write(
                root.path().join(".cargo/config.toml"),
                "[build]\ntarget-dir='relocated-build'\n",
            )?;
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let payload: Value = serde_json::from_slice(&std::fs::read(
            root.path().join("target/analysis/frame_metrics.json"),
        )?)?;
        assert!(
            std::path::Path::new(payload["meta"]["probe_executable"].as_str().unwrap())
                .starts_with(&destination)
        );
    }
    Ok(())
}

#[test]
fn unverified_criterion_cannot_supply_evidence_or_pass_fail_on_slow() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let estimates = root
        .path()
        .join("target/criterion/search_active_file_size/16384/new");
    std::fs::create_dir_all(&estimates)?;
    std::fs::write(estimates.join("estimates.json"), "{truncated")?;
    std::fs::create_dir(root.path().join("benches"))?;
    std::fs::write(
        root.path().join("benches/search_benchmark_targets.json"),
        json!({
            "search_active_file_size": {"workload_family":"search", "threshold_ms":50}
        })
        .to_string(),
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_splens"))
        .args(["measure", "search", "--skip-bench", "--fail-on-slow"])
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(root.path())
        .output()?;
    assert!(!output.status.success());
    let rows: Value = serde_json::from_slice(&std::fs::read(
        root.path().join("target/analysis/search_speed.json"),
    )?)?;
    assert_eq!(rows, json!([]));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("no current verified Criterion run manifest"));
    let provenance: Value = serde_json::from_slice(&std::fs::read(
        root.path()
            .join("target/analysis/search_speed.json.provenance.json"),
    )?)?;
    assert_eq!(provenance["status"], "failed");
    Ok(())
}
