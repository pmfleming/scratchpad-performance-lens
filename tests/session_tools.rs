use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

fn archive(path: &Path, mean: u64, other_health: &str) -> anyhow::Result<()> {
    fs::create_dir_all(path.join("performance"))?;
    fs::write(path.join("check.exit"), "0\n")?;
    let rows: Vec<_> = [
        "session_persist_latency/10000",
        "session_restore_latency/10000",
        "session_restore_background_completion/10000",
    ]
    .into_iter()
    .map(|name| {
        json!({
            "name": name, "measurement_status": "valid", "threshold_authoritative": true,
            "budget_status": "pass", "mean_ns": mean,
            "mean_confidence_interval_ns": {"upper_bound": mean + 10},
        })
    })
    .collect();
    let report = json!({"scenarios": [
        {"title": "Session Persistence Restore", "promise_health": "pass", "evidence": {"latency": rows}},
        {"title": "Other", "promise_health": other_health},
    ]});
    Ok(fs::write(
        path.join("performance/performance_review.json"),
        report.to_string(),
    )?)
}

#[test]
fn both_entrypoints_compare_archives_and_enforce_opt_in_promise_gate() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let baseline = dir.path().join("baseline");
    let current = dir.path().join("current");
    archive(&baseline, 100, "pass")?;
    archive(&current, 90, "at_risk")?;
    for bin in [
        env!("CARGO_BIN_EXE_splens"),
        env!("CARGO_BIN_EXE_scratchpad-performance-lens"),
    ] {
        for require_all in [false, true] {
            let mut command = Command::new(bin);
            command
                .arg("compare-session-baseline")
                .arg(&baseline)
                .arg(&current);
            if require_all {
                command.arg("--require-all-promises");
            }
            let output = command.output()?;
            assert_eq!(
                output.status.success(),
                !require_all,
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("Session latency acceptance: PASS"));
            assert!(stdout.contains("All-project promise compliance: NOT MET"));
            assert!(stdout.contains("0.900x baseline"));
        }
    }
    archive(&current, 101, "pass")?;
    let output = Command::new(env!("CARGO_BIN_EXE_splens"))
        .arg("compare-session-baseline")
        .arg(&baseline)
        .arg(&current)
        .output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Session latency acceptance: FAIL"));
    let tolerant = Command::new(env!("CARGO_BIN_EXE_splens"))
        .arg("compare-session-baseline")
        .arg(&baseline)
        .arg(&current)
        .args(["--max-regression-percent", "2"])
        .output()?;
    assert!(tolerant.status.success());
    assert!(String::from_utf8_lossy(&tolerant.stdout).contains("Allowed latency regression: 2.00%"));
    Ok(())
}

#[test]
fn dirty_save_cli_requires_explicit_probe_and_valid_launch_count_before_writing() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("archive");
    for args in [
        vec![],
        vec!["--binary", "missing", "--launches", "0"],
        vec!["--binary", "missing", "--launches", "21"],
        vec!["--binary", "missing", "--launches", "-1"],
        vec!["--binary", "missing", "--launches", "1"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_splens"))
            .arg("measure-large-dirty-save")
            .args(args)
            .arg("--output")
            .arg(&output)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!output.exists());
    }
}

fn write_measurement_contract(root: &std::path::Path) -> anyhow::Result<()> {
    let scenarios = [
        "stored_usage",
        "closed_token_lookup",
        "closed_archive_append",
        "save_after_branch",
        "save_after_navigation",
        "incremental_save",
    ];
    fs::create_dir_all(root.join("benches"))?;
    fs::write(root.join("benches/measurement_manifest.json"),json!({
        "fixtures":{"large_dirty_buffer_save":{"equals":{"fixture_revision":2,"history_retention":"unbounded"},"minimum":{"history_payload_bytes":52428800}}},
        "diagnostics":{"session_maintenance":{"samples_per_scenario":9,
            "fixture":{"equals":{"fixture_revision":1,"buffer_bytes":52428800,"initial_archives":128,"synchronous":"FULL"}},
            "scenarios":scenarios.iter().map(|id|json!({"id":id,"save_required":id.starts_with("save_after_") || *id == "incremental_save"})).collect::<Vec<_>>()}}
    }).to_string())?;
    Ok(())
}

#[cfg(unix)]
fn measurement_command(kind: &str, binary: &Path, archive: &Path, launches: u8) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_splens"));
    command
        .arg(kind)
        .arg("--binary")
        .arg(binary)
        .arg("--output")
        .arg(archive)
        .arg("--launches")
        .arg(launches.to_string());
    command
}

#[cfg(unix)]
#[test]
fn maintenance_cli_runs_the_selected_probe_and_requires_all_scenarios() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    write_measurement_contract(dir.path())?;
    let binary = dir.path().join("maintenance probe");
    let samples = ["stored_usage", "closed_token_lookup", "closed_archive_append", "save_after_branch", "save_after_navigation", "incremental_save"]
        .iter().flat_map(|scenario| (0..9).map(move |step| json!({
            "scenario": scenario, "step_index": step, "status": "ok", "measurement_scope": "prepared_operation",
            "workload_details": {"fixture_revision": 1, "buffer_bytes": 50*1024*1024, "initial_archives": 128, "synchronous": "FULL"},
            "phase_elapsed_ns": {"action_ns": 1_000_000, "save_ns": 2_000_000}, "elapsed_ns": 3_000_000,
            "result_unit": "snapshot-bytes-hashed", "result_value": 65536, "process_resources": {"write_bytes": null}
        }).to_string())).collect::<Vec<_>>().join("\n");
    fs::write(&binary, format!("#!/bin/sh\ntest \"$1\" = --session-maintenance || exit 2\ncat <<'SAMPLES'\n{samples}\nSAMPLES\n"))?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    let archive = dir.path().join("archive");
    let mut command = measurement_command("measure-session-maintenance", &binary, &archive, 1);
    let output = command.output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(summary[0]["scenarios"].as_array().unwrap().len(), 6);
    assert_eq!(summary[0]["launch"], 0);
    for row in summary[0]["scenarios"].as_array().unwrap() {
        assert_eq!(row["median_elapsed_ms"], 3.0);
        assert_eq!(row["median_snapshot_bytes_hashed"], 65536);
        assert!(row["median_write_bytes"].is_null());
    }
    let metadata: Value = serde_json::from_slice(&fs::read(archive.join("metadata.json"))?)?;
    assert_eq!(metadata["probe_flag"], "--session-maintenance");
    assert!(!command.output()?.status.success());
    Ok(())
}

#[cfg(unix)]
#[test]
fn dirty_save_cli_archives_independent_launches_and_refuses_overwrite() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    write_measurement_contract(dir.path())?;
    let binary = dir.path().join("probe with spaces");
    let samples = (0..9)
        .map(|step| {
            json!({
        "scenario": "large_dirty_buffer_save", "step_index": step, "status": "ok",
        "measurement_scope":"prepared_operation","workload_details":{"fixture_revision":2,"history_retention":"unbounded","history_payload_bytes":52428800},
        "phase_elapsed_ns": {"save_ns": 1_000_000}, "process_resources": {"write_bytes": null},
    }).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let pids_path = dir.path().join("pids");
    fs::write(&binary, format!("#!/bin/sh\ntest \"$1\" = --large-dirty-save || exit 2\necho $$ >> '{}'\nprintf diagnostic >&2\ncat <<'SAMPLES'\n{samples}\nSAMPLES\n", pids_path.display()))?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    let archive = dir.path().join("archive");
    let mut command = measurement_command("measure-large-dirty-save", &binary, &archive, 3);
    let output = command.output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Vec<Value> = serde_json::from_slice(&output.stdout)?;
    assert_eq!(summary.len(), 3);
    for (launch, row) in summary.iter().enumerate() {
        assert_eq!(
            *row,
            json!({"launch": launch, "median_save_ms": 1.0, "median_write_bytes": null, "write_bytes_available": false})
        );
        assert!(archive.join(format!("run-{launch}.jsonl")).is_file());
        assert_eq!(
            fs::read_to_string(archive.join(format!("run-{launch}.stderr")))?,
            "diagnostic"
        );
    }
    assert_eq!(
        summary,
        serde_json::from_slice::<Vec<Value>>(&fs::read(archive.join("summary.json"))?)?
    );
    let metadata: Value = serde_json::from_slice(&fs::read(archive.join("metadata.json"))?)?;
    assert_eq!(metadata["launches"], 3);
    assert_eq!(
        metadata["binary_sha256"],
        format!("{:x}", Sha256::digest(fs::read(&binary)?))
    );
    let pids = fs::read_to_string(&pids_path)?;
    assert_eq!(
        pids.lines().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    assert!(!command.output()?.status.success());
    assert_eq!(fs::read_to_string(&pids_path)?, pids);
    Ok(())
}
