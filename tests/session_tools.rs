use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

fn archive(path: &Path, mean: u64, other_health: &str) {
    fs::create_dir_all(path.join("performance")).unwrap();
    fs::write(path.join("check.exit"), "0\n").unwrap();
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
    fs::write(
        path.join("performance/performance_review.json"),
        report.to_string(),
    )
    .unwrap();
}

#[test]
fn both_entrypoints_compare_archives_and_enforce_opt_in_promise_gate() {
    let dir = tempfile::tempdir().unwrap();
    let baseline = dir.path().join("baseline");
    let current = dir.path().join("current");
    archive(&baseline, 100, "pass");
    archive(&current, 90, "at_risk");
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
            let output = command.output().unwrap();
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
    archive(&current, 101, "pass");
    let output = Command::new(env!("CARGO_BIN_EXE_splens"))
        .arg("compare-session-baseline")
        .arg(&baseline)
        .arg(&current)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Session latency acceptance: FAIL"));
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
fn dirty_save_cli_writes_compatible_archive_and_prints_summary() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
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
    fs::write(&binary, format!("#!/bin/sh\ntest \"$1\" = --large-dirty-save || exit 2\ncat <<'SAMPLES'\n{samples}\nSAMPLES\n")).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let archive = dir.path().join("archive");
    let output = Command::new(env!("CARGO_BIN_EXE_splens"))
        .arg("measure-large-dirty-save")
        .arg("--binary")
        .arg(&binary)
        .arg("--output")
        .arg(&archive)
        .args(["--launches", "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        summary,
        json!([{"launch": 0, "median_save_ms": 1.0, "median_write_bytes": null, "write_bytes_available": false}])
    );
    assert_eq!(
        summary,
        serde_json::from_slice::<Value>(&fs::read(archive.join("summary.json")).unwrap()).unwrap()
    );
    assert!(archive.join("metadata.json").is_file());
    assert!(archive.join("run-0.jsonl").is_file());
    assert!(archive.join("run-0.stderr").is_file());
}
