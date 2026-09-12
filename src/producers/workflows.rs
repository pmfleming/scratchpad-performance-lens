use super::common::{probe_path, run_probe_object, write_probe_artifact};
use crate::{cli::MeasureOptions, config::LensConfig};
use anyhow::Result;
use serde_json::json;

pub fn search_workflows(config: &LensConfig, _: MeasureOptions) -> Result<()> {
    let result = run_probe_object(&config.project_root,
        &["cargo", "build", "--release", "--quiet", "--bin", "search_workflow_metrics"],
        probe_path("search_workflow_metrics"));
    let (payload, failure) = match result {
        Ok(payload) if payload["scenarios"].as_array().is_some_and(|rows| rows.len() == 12) => (payload, None),
        Ok(_) => (json!({"meta":{"probe_status":"failed"},"scenarios":[]}), Some("incomplete search workflow probe".into())),
        Err(error) => (json!({"meta":{"probe_status":"failed","error":error.to_string()},"scenarios":[]}), Some(error.to_string())),
    };
    write_probe_artifact(&config.output_dir.join("search_workflow_metrics.json"), &payload,
        "search workflows", "Production query dispatch, delivery, render preparation and completion (no present).".into(), failure)
}
