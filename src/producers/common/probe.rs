use crate::shared;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(in crate::producers) fn probe_path(bin: &str) -> PathBuf {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    PathBuf::from("target")
        .join("release")
        .join(format!("{bin}{suffix}"))
}

pub(in crate::producers) fn run_probe_object(project_root: &Path, bin: &str) -> Result<Value> {
    let executable = build_probe(project_root, bin)?;
    let (output, timed_out) = bounded_output(project_root, &executable, &[])?;
    anyhow::ensure!(!timed_out, "probe timed out");
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    let mut payload: Value = serde_json::from_slice(&output.stdout)?;
    if payload.is_object() {
        if payload["meta"].is_null() {
            payload["meta"] = serde_json::json!({});
        }
        anyhow::ensure!(payload["meta"].is_object(), "probe meta must be an object");
        payload["meta"]["probe_executable"] = serde_json::json!(executable);
        Ok(payload)
    } else {
        bail!("probe returned a non-object payload")
    }
}

pub(in crate::producers) struct ProbeEvents {
    pub events: Vec<Value>,
    pub failure: Option<String>,
}

pub(in crate::producers) fn run_sweeps(
    root: &Path,
    bin: &str,
    scenarios: &[&str],
) -> Result<ProbeEvents> {
    let executable = build_probe(root, bin)?;
    let manifest = std::fs::read(root.join("benches/measurement_manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or_default();
    Ok(merge_runs(scenarios.iter().map(|scenario| {
        scenario_runs(root, &executable, bin, scenario, &manifest)
            .map(|mut result| {
                result.failure = result.failure.map(|error| format!("{scenario}: {error}"));
                result
            })
            .map_err(|error| anyhow::anyhow!("{scenario}: {error}"))
    })))
}

fn scenario_runs(
    root: &Path,
    executable: &Path,
    bin: &str,
    scenario: &str,
    manifest: &Value,
) -> Result<ProbeEvents> {
    let minimum = minimum_launches(manifest, bin, scenario)?;
    let run = || run_events(root, executable, &["--scenario", scenario], scenario);
    let first = run()?;
    let launches = if near_budget(manifest, bin, scenario, &first.events) {
        3
    } else {
        1
    }
    .max(minimum);
    let runs = std::iter::once(Ok(first)).chain((1..launches).map(|_| run()));
    Ok(merge_runs(runs.enumerate().map(|(launch, result)| {
        result.map(|mut result| {
            annotate_launch(
                &mut result.events,
                launch,
                launches,
                bin == "capacity_probe",
            );
            result
        })
    })))
}

fn minimum_launches(manifest: &Value, bin: &str, scenario: &str) -> Result<usize> {
    let budgets = if bin == "capacity_probe" {
        "capacity_budgets"
    } else {
        "resource_budgets"
    };
    let Some(value) = manifest[budgets][scenario].get("min_independent_launches") else {
        return Ok(1);
    };
    let count = value
        .as_u64()
        .filter(|count| (1..=16).contains(count))
        .context("min_independent_launches must be an integer from 1 through 16")?;
    Ok(count as usize)
}

fn merge_runs(runs: impl IntoIterator<Item = Result<ProbeEvents>>) -> ProbeEvents {
    let mut events = Vec::new();
    let mut failures = Vec::new();
    for run in runs {
        match run {
            Ok(result) => {
                events.extend(result.events);
                failures.extend(result.failure);
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    ProbeEvents {
        events,
        failure: (!failures.is_empty()).then(|| failures.join("; ")),
    }
}

fn annotate_launch(events: &mut [Value], launch: usize, launches: usize, capacity: bool) {
    for event in events {
        event["launch_index"] = serde_json::json!(launch);
        event["independent_launch_count"] = serde_json::json!(launches);
        if capacity {
            let count = event["expected_repetition_count"].as_u64().unwrap_or(3);
            if let Some(index) = event["repeat_index"].as_u64() {
                event["repeat_index"] = serde_json::json!(index + count * launch as u64);
            }
            event["expected_repetition_count"] = serde_json::json!(count * launches as u64);
        }
    }
}

fn near_budget(manifest: &Value, bin: &str, scenario: &str, events: &[Value]) -> bool {
    let (limit, metric) = if bin == "capacity_probe" {
        (
            manifest["capacity_budgets"][scenario]["threshold_ms"]
                .as_f64()
                .map(|v| v * 1e6),
            "/elapsed_ns",
        )
    } else {
        (
            manifest["resource_budgets"][scenario]["max"].as_f64(),
            manifest["resource_budgets"][scenario]["metric"]
                .as_str()
                .unwrap_or("/elapsed_ns"),
        )
    };
    limit.filter(|v| *v > 0.0).is_some_and(|limit| {
        events.iter().any(|event| {
            event
                .pointer(metric)
                .and_then(Value::as_f64)
                .is_some_and(|v| (0.9..=1.1).contains(&(v / limit)))
        })
    })
}

fn bounded_output(
    root: &Path,
    executable: &Path,
    args: &[&str],
) -> Result<(std::process::Output, bool)> {
    // File-backed capture avoids pipe deadlocks and retains output after a kill.
    let archive = shared::cargo_target_directory(root).join("splens-probes");
    std::fs::create_dir_all(&archive)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let name = format!(
        "{}-{}-{stamp}",
        executable.file_stem().unwrap_or_default().to_string_lossy(),
        std::process::id()
    );
    let stdout_path = archive.join(format!("{name}.ndjson"));
    let stderr_path = archive.join(format!("{name}.stderr"));
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(root)
        .stdout(Stdio::from(std::fs::File::create(&stdout_path)?))
        .stderr(Stdio::from(std::fs::File::create(&stderr_path)?));
    let timeout = Duration::from_secs(
        std::env::var("SPLENS_PROBE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(180),
    );
    let exit = crate::process::run(&mut command, timeout)?;
    Ok((
        std::process::Output {
            status: exit.status,
            stdout: std::fs::read(stdout_path)?,
            stderr: std::fs::read(stderr_path)?,
        },
        exit.timed_out,
    ))
}

fn run_events(root: &Path, executable: &Path, args: &[&str], label: &str) -> Result<ProbeEvents> {
    let start = Instant::now();
    let (output, timed_out) =
        bounded_output(root, executable, args).with_context(|| label.to_owned())?;
    let mut result = parse_partial(&output.stdout);
    if timed_out || !output.status.success() {
        let reason = if timed_out {
            "timeout".to_owned()
        } else {
            format!(
                "exit {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )
        };
        if let Some(mut event) = result.1.take() {
            event["status"] = serde_json::json!("error");
            event["elapsed_ns"] =
                serde_json::json!(start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
            event["measurement_scope"] = serde_json::json!("failed_process_including_setup");
            event["note"] = serde_json::json!(reason);
            result.0.push(event);
        }
        result.2.push(reason);
    } else if result.1.is_some() {
        result.2.push("unfinished step".into());
    }
    if result.0.is_empty() {
        result.2.push("probe returned no measurements".into());
    }
    for event in &mut result.0 {
        event["probe_executable"] = serde_json::json!(executable);
    }
    Ok(ProbeEvents {
        events: result.0,
        failure: (!result.2.is_empty()).then(|| result.2.join("; ")),
    })
}

fn parse_partial(bytes: &[u8]) -> (Vec<Value>, Option<Value>, Vec<String>) {
    let mut events = Vec::new();
    let mut active = None;
    let mut errors = Vec::new();
    for (index, line) in bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .enumerate()
    {
        match serde_json::from_slice::<Value>(line) {
            Ok(event) if event["event"] == "step_started" => active = Some(event),
            Ok(event) if valid_event(&event) => {
                active = None;
                events.push(event);
            }
            Ok(_) => errors.push(format!(
                "invalid required measurement fields on probe line {}",
                index + 1
            )),
            Err(error) => errors.push(format!("line {}: {error}", index + 1)),
        }
    }
    (events, active, errors)
}

fn build_probe(root: &Path, bin: &str) -> Result<PathBuf> {
    let output = shared::run_command(
        root,
        &[
            "cargo",
            "build",
            "--release",
            "--quiet",
            "--bin",
            bin,
            "--message-format=json-render-diagnostics",
        ],
    )?;
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    executable_from_messages(root, &output.stdout, bin)
}

fn executable_from_messages(root: &Path, bytes: &[u8], bin: &str) -> Result<PathBuf> {
    let executable = String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| {
            message["reason"] == "compiler-artifact"
                && message["target"]["name"] == bin
                && message["target"]["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from))
        .next_back()
        .context(
            "Cargo reported no executable for the requested probe; refusing a guessed/stale binary",
        )?;
    Ok(if executable.is_absolute() {
        executable
    } else {
        root.join(executable)
    })
}

fn valid_event(event: &Value) -> bool {
    event.is_object()
        && event["scenario"].is_string()
        && event["workload_value"].as_u64().is_some()
        && event["step_index"].as_u64().is_some()
        && event["elapsed_ns"].as_u64().is_some_and(|ns| ns > 0)
        && matches!(event["status"].as_str(), Some("ok" | "panic" | "error"))
}

pub(in crate::producers) fn write_probe_artifact(
    output_path: &Path,
    payload: &Value,
    label: &str,
    cli_text: String,
    failure: Option<String>,
) -> Result<()> {
    shared::write_visibility(output_path, payload, label, cli_text)?;
    if let Some(error) = failure {
        bail!("{error}");
    }
    Ok(())
}

pub(in crate::producers) fn output_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn declared_minimum_runs_are_enforced_even_away_from_a_budget() {
        use serde_json::json;
        assert_eq!(
            super::minimum_launches(&json!({}), "resource_probe", "cold").unwrap(),
            1
        );
        let mut manifest = json!({"resource_budgets":{"cold":{"min_independent_launches":3}}});
        assert_eq!(
            super::minimum_launches(&manifest, "resource_probe", "cold").unwrap(),
            3
        );
        for invalid in [json!(0), json!(17), json!(1.5), json!(null)] {
            manifest["resource_budgets"]["cold"]["min_independent_launches"] = invalid;
            assert!(super::minimum_launches(&manifest, "resource_probe", "cold").is_err());
        }
    }
    #[test]
    fn executable_path_comes_from_matching_cargo_artifact_not_default_target() {
        let root = std::path::Path::new("/workspace");
        let messages = br#"{"reason":"compiler-artifact","target":{"name":"other","kind":["bin"]},"executable":"/wrong"}
{"reason":"compiler-artifact","target":{"name":"probe","kind":["bin"]},"executable":"custom-build/triple/release/probe"}"#;
        assert_eq!(
            super::executable_from_messages(root, messages, "probe").unwrap(),
            root.join("custom-build/triple/release/probe")
        );
        assert!(super::executable_from_messages(root, b"", "probe").is_err());
    }
}
