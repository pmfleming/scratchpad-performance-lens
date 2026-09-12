use crate::shared;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let build = shared::run_command(project_root, build_cmd)?;
    if !build.status.success() {
        bail!("{}", output_text(&build));
    }
    let output = Command::new(project_root.join(probe_path))
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    let payload: Value = serde_json::from_slice(&output.stdout)?;
    if payload.is_object() {
        Ok(payload)
    } else {
        bail!("probe returned a non-object payload")
    }
}

pub(in crate::producers) fn run_probe_events(
    project_root: &Path,
    build_cmd: &[&str],
    probe_path: PathBuf,
    label: &str,
) -> Result<Vec<Value>> {
    let build = shared::run_command(project_root, build_cmd)?;
    if !build.status.success() {
        bail!("{}", output_text(&build));
    }
    let output = Command::new(project_root.join(probe_path))
        .current_dir(project_root)
        .output()
        .with_context(|| label.to_string())?;
    // Preserve even a crashed/truncated probe's evidence for diagnosis.
    let archive = project_root.join("target/splens-probes");
    std::fs::create_dir_all(&archive)?;
    let run = format!("{}-{}", label.replace(' ', "-"), crate::provenance::timestamp());
    std::fs::write(archive.join(format!("{run}.ndjson")), &output.stdout)?;
    std::fs::write(archive.join(format!("{run}.stderr")), &output.stderr)?;
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    parse_events(&output.stdout)
}

fn parse_events(bytes: &[u8]) -> Result<Vec<Value>> {
    let text = std::str::from_utf8(bytes).context("probe output is not UTF-8")?;
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate().filter(|(_, line)| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line).with_context(|| format!("invalid probe line {}", index + 1))?;
        if !event.is_object() || event["scenario"].as_str().is_none()
            || event["workload_value"].as_u64().is_none()
            || event["step_index"].as_u64().is_none()
            || event["elapsed_ns"].as_u64().is_none_or(|value| value == 0)
            || !matches!(event["status"].as_str(), Some("ok" | "panic" | "error")) {
            bail!("invalid required measurement fields on probe line {}", index + 1);
        }
        events.push(event);
    }
    if events.is_empty() { bail!("probe returned no measurements"); }
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

#[cfg(test)]
mod tests {
    use super::parse_events;
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

pub(in crate::producers) fn output_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
