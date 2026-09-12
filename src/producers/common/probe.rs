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
    let executable = build_probe(project_root, build_cmd, &probe_path)?;
    let output = Command::new(&executable)
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    let mut payload: Value = serde_json::from_slice(&output.stdout)?;
    if payload.is_object() {
        if payload["meta"].is_null() { payload["meta"] = serde_json::json!({}); }
        anyhow::ensure!(payload["meta"].is_object(), "probe meta must be an object");
        payload["meta"]["probe_executable"] = serde_json::json!(executable);
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
    let executable = build_probe(project_root, build_cmd, &probe_path)?;
    let output = Command::new(&executable)
        .current_dir(project_root)
        .output()
        .with_context(|| label.to_string())?;
    // Preserve even a crashed/truncated probe's evidence for diagnosis.
    let archive = super::stats::criterion_dir(project_root).parent().unwrap().join("splens-probes");
    std::fs::create_dir_all(&archive)?;
    let run = format!("{}-{}", label.replace(' ', "-"), crate::provenance::timestamp());
    std::fs::write(archive.join(format!("{run}.ndjson")), &output.stdout)?;
    std::fs::write(archive.join(format!("{run}.stderr")), &output.stderr)?;
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    let mut events = parse_events(&output.stdout)?;
    for event in &mut events { event["probe_executable"] = serde_json::json!(executable); }
    Ok(events)
}

fn build_probe(root: &Path, args: &[&str], requested: &Path) -> Result<PathBuf> {
    let bin = requested.file_stem().and_then(|name| name.to_str()).context("missing probe target name")?;
    let mut args = args.to_vec();
    args.push("--message-format=json-render-diagnostics");
    let output = shared::run_command(root, &args)?;
    if !output.status.success() { bail!("{}", output_text(&output)); }
    executable_from_messages(root, &output.stdout, bin)
}

fn executable_from_messages(root: &Path, bytes: &[u8], bin: &str) -> Result<PathBuf> {
    let executable = String::from_utf8_lossy(bytes).lines().filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["reason"] == "compiler-artifact" && message["target"]["name"] == bin
            && message["target"]["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin")))
        .filter_map(|message| message["executable"].as_str().map(PathBuf::from)).last()
        .context("Cargo reported no executable for the requested probe; refusing a guessed/stale binary")?;
    Ok(if executable.is_absolute() { executable } else { root.join(executable) })
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
    fn executable_path_comes_from_matching_cargo_artifact_not_default_target() {
        let root = std::path::Path::new("/workspace");
        let messages = br#"{"reason":"compiler-artifact","target":{"name":"other","kind":["bin"]},"executable":"/wrong"}
{"reason":"compiler-artifact","target":{"name":"probe","kind":["bin"]},"executable":"custom-build/triple/release/probe"}"#;
        assert_eq!(super::executable_from_messages(root, messages, "probe").unwrap(), root.join("custom-build/triple/release/probe"));
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

pub(in crate::producers) fn output_text(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
