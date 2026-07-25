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
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(Value::is_object)
        .collect())
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
