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

#[cfg(unix)]
#[test]
fn dirty_save_cli_archives_independent_launches_and_refuses_overwrite() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let binary = dir.path().join("probe with spaces");
    let samples = (0..9)
        .map(|step| {
            json!({
        "scenario": "large_dirty_buffer_save", "step_index": step, "status": "ok",
        "phase_elapsed_ns": {"save_ns": 1_000_000}, "process_resources": {"write_bytes": null},
    }).to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let pids_path = dir.path().join("pids");
    fs::write(&binary, format!("#!/bin/sh\ntest \"$1\" = --large-dirty-save || exit 2\necho $$ >> '{}'\nprintf diagnostic >&2\ncat <<'SAMPLES'\n{samples}\nSAMPLES\n", pids_path.display()))?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    let archive = dir.path().join("archive");
    let mut command = Command::new(env!("CARGO_BIN_EXE_splens"));
    let output = command
        .arg("measure-large-dirty-save")
        .arg("--binary")
        .arg(&binary)
        .arg("--output")
        .arg(&archive)
        .args(["--launches", "3"])
        .output()?;
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
