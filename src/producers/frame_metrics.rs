use super::common::{probe_path, run_probe_object};
use super::render::render_frame_metrics;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{bail, Result};
use serde_json::{json, Value};
pub fn frame_metrics(config: &LensConfig) -> Result<()> {
    let payload = match run_probe_object(
        &config.project_root,
        &[
            "cargo",
            "build",
            "--release",
            "--quiet",
            "--bin",
            "frame_metrics",
        ],
        probe_path("frame_metrics"),
    ) {
        Ok(mut value) => {
            value["meta"]["generated_from"] = json!("rust:frame_metrics");
            value["meta"]["probe_status"] = json!("completed");
            value
        }
        Err(error) => json!({
            "meta": {"generated_from": "rust:frame_metrics", "probe_status": "failed", "error": error.to_string()},
            "scenarios": [],
        }),
    };
    let failed = payload
        .get("meta")
        .and_then(|meta| meta.get("probe_status"))
        .and_then(Value::as_str)
        == Some("failed");
    shared::write_visibility(
        &config.output_dir.join("frame_metrics.json"),
        &payload,
        "frame metrics",
        render_frame_metrics(&payload),
    )?;
    if failed {
        bail!("frame metrics probe failed");
    }
    Ok(())
}
