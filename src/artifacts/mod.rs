pub mod capacity_report;
pub mod performance_review;

use anyhow::Result;
use serde_json::json;
use std::path::Path;

pub(crate) fn export_schemas(output: &Path) -> Result<()> {
    std::fs::create_dir_all(output)?;
    let schemas = [
        (
            "residency_ladder",
            "residency_ladder.schema.json",
            "residency_ladder.json",
            serde_json::to_value(schemars::schema_for!(crate::residency::LadderArtifact))?,
        ),
        (
            "residency_trial",
            "residency_trial.schema.json",
            "residency trial input",
            serde_json::to_value(schemars::schema_for!(crate::residency::Trial))?,
        ),
        (
            "outcome",
            "outcome.schema.json",
            "manifest outcome",
            serde_json::to_value(schemars::schema_for!(crate::outcomes::Outcome))?,
        ),
        (
            performance_review::SCHEMA_ID,
            performance_review::SCHEMA_FILE,
            "performance_review.json",
            serde_json::to_value(schemars::schema_for!(
                performance_review::PerformanceReviewArtifact
            ))?,
        ),
        (
            capacity_report::SCHEMA_ID,
            capacity_report::SCHEMA_FILE,
            "capacity_report.json",
            serde_json::to_value(schemars::schema_for!(
                capacity_report::CapacityReportArtifact
            ))?,
        ),
    ];
    for (_, file, _, schema) in &schemas {
        std::fs::write(
            output.join(file),
            format!("{}\n", serde_json::to_string_pretty(schema)?),
        )?;
    }
    let index = json!({"version": 1, "schemas": schemas.iter().map(|(id, file, artifact, _)| json!({
        "id": id, "file": file, "artifact": artifact,
    })).collect::<Vec<_>>()});
    std::fs::write(
        output.join("index.json"),
        format!("{}\n", serde_json::to_string_pretty(&index)?),
    )?;
    println!("Wrote schemas to {}", output.display());
    Ok(())
}
