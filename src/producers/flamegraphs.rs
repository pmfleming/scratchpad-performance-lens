use super::render::render_flamegraphs;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{bail, Result};
use serde_json::Value;
use std::path::Path;
use std::process::Command;
pub fn flamegraphs(config: &LensConfig, options: MeasureOptions) -> Result<()> {
    let output_dir = config.output_dir.join("flamegraphs");
    let mut issue = None;
    if !options.index_only {
        let tool = Command::new("cargo")
            .args(["flamegraph", "--version"])
            .current_dir(&config.project_root)
            .output();
        if tool
            .as_ref()
            .map_or(true, |output| !output.status.success())
        {
            issue = Some("cargo-flamegraph is not installed; coverage is indexed but new SVGs were not generated.".to_string());
        } else {
            std::fs::create_dir_all(&output_dir)?;
            for config_row in shared::FLAMEGRAPH_CONFIGS {
                let svg_path = output_dir.join(format!("{}.svg", config_row.id));
                eprintln!("Generating flamegraph for {}...", config_row.name);
                let mut command = Command::new("cargo");
                command.args(["flamegraph", "--dev", "-o"]);
                command.arg(&svg_path);
                command.args(config_row.cargo_args);
                let output = command.current_dir(&config.project_root).output()?;
                if !output.status.success() {
                    let text = format!(
                        "{}{}",
                        String::from_utf8_lossy(&output.stderr),
                        String::from_utf8_lossy(&output.stdout)
                    );
                    issue = Some(format!("Generation failed: {}", text.trim()));
                    break;
                }
            }
        }
    }
    let rows: Vec<_> = shared::FLAMEGRAPH_CONFIGS
        .iter()
        .map(|config_row| shared::flamegraph_json(config_row, &output_dir, issue.as_deref()))
        .collect();
    let payload = Value::Array(rows);
    shared::write_visibility(
        &config.output_dir.join("flamegraphs.json"),
        &payload,
        "flamegraph index",
        render_flamegraphs(&payload),
    )?;
    if let Some(error) = issue {
        bail!("flamegraph probe failed: {error}");
    }
    Ok(())
}

pub(super) fn fallback_flamegraphs(output_dir: &Path) -> Value {
    Value::Array(
        shared::FLAMEGRAPH_CONFIGS
            .iter()
            .map(|config| shared::flamegraph_json(config, output_dir, None))
            .collect(),
    )
}
