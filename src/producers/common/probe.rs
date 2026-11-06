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

pub(in crate::producers) fn run_probe_object(
    project_root: &Path,
    build_cmd: &[&str],
    probe_path: PathBuf,
) -> Result<Value> {
    let executable = build_probe(project_root, build_cmd, &probe_path)?;
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

pub(in crate::producers) fn run_capacity_sweeps(
    root: &Path,
    scenarios: &[&str],
) -> Result<ProbeEvents> {
    run_sweeps(root, "capacity_probe", scenarios)
}
pub(in crate::producers) fn run_sweeps(
    root: &Path,
    bin: &str,
    scenarios: &[&str],
) -> Result<ProbeEvents> {
    let executable = build_probe(
        root,
        &["cargo", "build", "--release", "--quiet", "--bin", bin],
        &probe_path(bin),
    )?;
    let mut combined = ProbeEvents {
        events: Vec::new(),
        failure: None,
    };
    let mut failures = Vec::new();
    for scenario in scenarios {
        let mut runs = Vec::new();
        match run_events(root, &executable, &["--scenario", scenario], scenario) {
            Ok(result) => {
                let repeat = near_budget(root, bin, scenario, &result.events);
                runs.push(result);
                if repeat {
                    for _ in 0..2 {
                        match run_events(root, &executable, &["--scenario", scenario], scenario) {
                            Ok(result) => runs.push(result),
                            Err(error) => failures.push(format!("{scenario}: {error}")),
                        }
                    }
                }
                let launch_count = runs.len();
                for (launch, mut result) in runs.into_iter().enumerate() {
                    for event in &mut result.events {
                        event["launch_index"] = serde_json::json!(launch);
                        event["independent_launch_count"] = serde_json::json!(launch_count);
                        if bin == "capacity_probe" {
                            let count = event["expected_repetition_count"].as_u64().unwrap_or(3);
                            if let Some(index) = event["repeat_index"].as_u64() {
                                event["repeat_index"] =
                                    serde_json::json!(index + count * launch as u64);
                            }
                            event["expected_repetition_count"] =
                                serde_json::json!(count * launch_count as u64);
                        }
                    }
                    combined.events.extend(result.events);
                    if let Some(error) = result.failure {
                        failures.push(format!("{scenario}: {error}"));
                    }
                }
            }
            Err(error) => failures.push(format!("{scenario}: {error}")),
        }
    }
    combined.failure = (!failures.is_empty()).then(|| failures.join("; "));
    Ok(combined)
}

fn near_budget(root: &Path, bin: &str, scenario: &str, events: &[Value]) -> bool {
    let manifest: Value = std::fs::read(root.join("benches/measurement_manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
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
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let timeout = Duration::from_secs(
        std::env::var("SPLENS_PROBE_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(180),
    );
    let start = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            timed_out = true;
            #[cfg(unix)]
            {
                let _ = Command::new("kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .status();
            }
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok((
        std::process::Output {
            status,
            stdout: std::fs::read(stdout_path)?,
            stderr: std::fs::read(stderr_path)?,
        },
        timed_out,
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
            Ok(event) => match parse_events(&serde_json::to_vec(&event).unwrap()) {
                Ok(mut rows) => {
                    active = None;
                    events.append(&mut rows);
                }
                Err(error) => errors.push(format!("line {}: {error}", index + 1)),
            },
            Err(error) => errors.push(format!("line {}: {error}", index + 1)),
        }
    }
    (events, active, errors)
}

fn build_probe(root: &Path, args: &[&str], requested: &Path) -> Result<PathBuf> {
    let bin = requested
        .file_stem()
        .and_then(|name| name.to_str())
        .context("missing probe target name")?;
    let mut args = args.to_vec();
    args.push("--message-format=json-render-diagnostics");
    let output = shared::run_command(root, &args)?;
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

fn parse_events(bytes: &[u8]) -> Result<Vec<Value>> {
    let text = std::str::from_utf8(bytes).context("probe output is not UTF-8")?;
    let mut events = Vec::new();
    for (index, line) in text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
    {
        let event: Value = serde_json::from_str(line)
            .with_context(|| format!("invalid probe line {}", index + 1))?;
        if !event.is_object()
            || event["scenario"].as_str().is_none()
            || event["workload_value"].as_u64().is_none()
            || event["step_index"].as_u64().is_none()
            || event["elapsed_ns"].as_u64().is_none_or(|value| value == 0)
            || !matches!(event["status"].as_str(), Some("ok" | "panic" | "error"))
        {
            bail!(
                "invalid required measurement fields on probe line {}",
                index + 1
            );
        }
        events.push(event);
    }
    if events.is_empty() {
        bail!("probe returned no measurements");
    }
    Ok(events)
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
    use super::parse_events;
    #[cfg(unix)]
    #[test]
    fn process_failure_keeps_rows_marks_active_workload_and_allows_next_probe() {
        let root = tempfile::tempdir().unwrap();
        let shell = std::path::PathBuf::from("/run/current-system/sw/bin/sh");
        let shell = if shell.exists() {
            shell
        } else {
            "/bin/sh".into()
        };
        let script = r#"printf '%s\n' '{"scenario":"test","step_index":0,"workload_value":1,"elapsed_ns":10,"status":"ok"}' '{"event":"step_started","scenario":"test","step_index":1,"workload_value":2}'; exit 1"#;
        let result = super::run_events(root.path(), &shell, &["-c", script], "crash").unwrap();
        assert!(result.failure.is_some());
        assert_eq!(result.events.len(), 2);
        assert_eq!(result.events[1]["status"], "error");
        assert_eq!(result.events[1]["workload_value"], 2);
        let next = super::run_events(root.path(), &shell, &["-c", "printf '%s\\n' '{\"scenario\":\"next\",\"step_index\":0,\"workload_value\":1,\"elapsed_ns\":10,\"status\":\"ok\"}'"], "next").unwrap();
        assert!(next.failure.is_none());
    }

    #[test]
    fn preserves_completed_rows_and_active_step_before_truncation() {
        let bytes =
            br#"{"scenario":"test","step_index":0,"workload_value":1,"elapsed_ns":10,"status":"ok"}
{"event":"step_started","scenario":"test","step_index":1,"workload_value":2}
{truncated"#;
        let (events, active, errors) = super::parse_partial(bytes);
        assert_eq!(events.len(), 1);
        assert_eq!(active.unwrap()["workload_value"], 2);
        assert_eq!(errors.len(), 1);
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

    #[test]
    fn malformed_partial_or_missing_fields_never_silently_succeed() {
        let valid = br#"{"scenario":"test","step_index":0,"workload_value":1,"elapsed_ns":10,"status":"ok"}"#;
        assert_eq!(parse_events(valid).unwrap().len(), 1);
        let mut truncated = valid.to_vec();
        truncated.extend_from_slice(b"\n{truncated");
        assert!(parse_events(&truncated).is_err());
        assert!(parse_events(b"{}").is_err());
        assert!(parse_events(b"").is_err());
    }
}
