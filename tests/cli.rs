use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(bin: &str, root: &Path, args: &[&str]) -> Output {
    Command::new(bin)
        .args(args)
        .current_dir(root)
        .output()
        .expect("lens should run")
}

fn payload(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn both_entrypoints_expose_catalog_and_telemetry_contracts() {
    let root = tempfile::tempdir().unwrap();
    for bin in [
        env!("CARGO_BIN_EXE_splens"),
        env!("CARGO_BIN_EXE_scratchpad-performance-lens"),
    ] {
        let catalog = payload(&run(bin, root.path(), &["catalog"]));
        let ids: Vec<_> = catalog["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|task| task["id"].as_str())
            .collect();
        for id in [
            "performance.search",
            "performance.performance-review",
            "telemetry.app-package",
        ] {
            assert!(ids.contains(&id));
        }
        let telemetry = payload(&run(bin, root.path(), &["telemetry"]));
        assert!(telemetry.get("manifest").is_some());
    }
}

#[test]
fn project_code_metrics_uses_a_self_contained_git_history() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join("src"))?;
    std::fs::write(root.path().join("src/main.rs"), "fn main() {}\n")?;
    for args in [
        vec!["init"],
        vec!["add", "."],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-m",
            "fixture",
        ],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(root.path())
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = run(
        env!("CARGO_BIN_EXE_splens"),
        root.path(),
        &["measure", "project-code"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&std::fs::read(
        root.path()
            .join("target/analysis/project_code_metrics.json"),
    )?)?;
    assert_eq!(report["current"]["application"], 1);
    assert_eq!(report["history"].as_array().unwrap().len(), 1);
    Ok(())
}

#[test]
fn schema_export_writes_typed_artifact_contracts() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let output = run(
        env!("CARGO_BIN_EXE_splens"),
        root.path(),
        &["schema", "export"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let index: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("schemas/index.json"))?)?;
    for (id, required) in [
        (
            "performance_review",
            vec!["promise_health", "failed_before_promise"],
        ),
        (
            "capacity_report",
            vec!["first_failure_workload", "workload_value"],
        ),
    ] {
        assert!(index["schemas"]
            .as_array()
            .unwrap()
            .iter()
            .any(|schema| schema["id"] == id));
        let schema =
            std::fs::read_to_string(root.path().join(format!("schemas/{id}.schema.json")))?;
        for field in required {
            assert!(schema.contains(field));
        }
    }
    Ok(())
}

#[test]
fn all_continues_after_failures_indexes_profiles_and_rejects_inapplicable_flags() {
    let root = tempfile::tempdir().unwrap();
    let profiles = root.path().join("target/analysis/flamegraphs");
    std::fs::create_dir_all(&profiles).unwrap();
    std::fs::write(profiles.join("tab_operations_profile.svg"), "<svg/>").unwrap();
    let output = run(
        env!("CARGO_BIN_EXE_splens"),
        root.path(),
        &[
            "measure",
            "all-with-flamegraphs",
            "--skip-bench",
            "--index-only",
        ],
    );
    assert!(!output.status.success());
    let analysis = root.path().join("target/analysis");
    assert!(analysis.join("performance_review.json").exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("measure producer(s) failed") && stderr.contains("frame-metrics"),
        "{stderr}"
    );
    let profiles: Value =
        serde_json::from_slice(&std::fs::read(analysis.join("flamegraphs.json")).unwrap()).unwrap();
    assert!(profiles
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["id"] == "tab_operations_profile" && row["available"] == true));
    for (tool, flag) in [
        ("capacity", "--fail-on-slow"),
        ("search", "--index-only"),
        ("unknown", "--skip-bench"),
    ] {
        let output = run(
            env!("CARGO_BIN_EXE_splens"),
            root.path(),
            &["measure", tool, flag],
        );
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("only applies") || stderr.contains("invalid value"),
            "{stderr}"
        );
    }
}
