use super::super::common::budget_probe_ms;
use super::registry::ReviewScenario;
use crate::artifacts::performance_review::SourceArtifactStatus;
use crate::config::LensConfig;
use crate::shared;
use serde_json::Value;
use std::collections::HashSet;
pub(super) fn review_row_matches(row: &Value, scenario: &ReviewScenario) -> bool {
    let key = row
        .get("benchmark_key")
        .or_else(|| row.get("name"))
        .and_then(Value::as_str)
        .map(shared::benchmark_key)
        .unwrap_or("");
    let family = row
        .get("workload_family")
        .or_else(|| row.get("family"))
        .and_then(Value::as_str)
        .unwrap_or("");
    scenario.benchmark_keys.contains(&key) || scenario.families.contains(&family)
}

pub(super) fn over_budget_latency(row: &&Value) -> bool {
    let budget_probe = budget_probe_ms(row);
    let threshold = row
        .get("threshold_ms")
        .or_else(|| row.get("budget_ms"))
        .and_then(Value::as_f64);
    matches!((budget_probe, threshold), (Some(value), Some(threshold)) if threshold > 0.0 && value > threshold)
}

pub(super) fn mean_ms(row: &Value) -> Option<f64> {
    row.get("mean_ms").and_then(Value::as_f64).or_else(|| {
        row.get("mean_ns")
            .and_then(Value::as_f64)
            .map(|value| value / 1_000_000.0)
    })
}

pub(super) fn unique_rows(rows: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|row| {
            let key = format!(
                "{}|{}|{}|{}",
                row.get("benchmark_key")
                    .or_else(|| row.get("scenario"))
                    .or_else(|| row.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                row.get("name").and_then(Value::as_str).unwrap_or(""),
                row.get("parameter_value")
                    .map(Value::to_string)
                    .unwrap_or_default(),
                row.get("scenario_label")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            );
            seen.insert(key)
        })
        .collect()
}

pub(super) fn payload_synthetic(payload: &Value) -> bool {
    payload
        .get("meta")
        .and_then(|meta| meta.get("synthetic"))
        .or_else(|| {
            payload
                .get("summary")
                .and_then(|summary| summary.get("synthetic"))
        })
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn source_status(config: &LensConfig) -> Vec<SourceArtifactStatus> {
    [
        ("slowspots", "slowspots.json"),
        ("frame_metrics", "frame_metrics.json"),
        ("search_speed", "search_speed.json"),
        ("capacity", "capacity_report.json"),
        ("resources", "resource_profiles.json"),
        ("flamegraphs", "flamegraphs.json"),
        ("speed_report", "speed_efficiency_report.json"),
    ]
    .into_iter()
    .map(|(id, file)| {
        let path = config.output_dir.join(file);
        let (available, status, probe_status, error) = match shared::read_json(&path) {
            shared::JsonRead::Loaded(payload) => {
                let probe_status = payload
                    .get("meta")
                    .and_then(|meta| meta.get("probe_status"))
                    .or_else(|| {
                        payload
                            .get("summary")
                            .and_then(|summary| summary.get("probe_status"))
                    })
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                (true, "loaded", probe_status, None)
            }
            shared::JsonRead::Missing => (false, "missing", None, None),
            shared::JsonRead::Unparseable(error) => (false, "unparseable", None, Some(error)),
        };
        SourceArtifactStatus {
            id: id.to_string(),
            path: path.to_string_lossy().to_string(),
            available,
            status: status.to_string(),
            probe_status,
            error,
        }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::source_status;
    use crate::config::LensConfig;

    #[test]
    fn source_status_distinguishes_loaded_missing_and_unparseable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("slowspots.json"), "[]").unwrap();
        std::fs::write(dir.path().join("frame_metrics.json"), "{truncated").unwrap();
        let config = LensConfig {
            project_name: "fixture".to_string(),
            project_root: dir.path().to_path_buf(),
            output_dir: dir.path().to_path_buf(),
        };

        let statuses = source_status(&config);
        let slowspots = statuses.iter().find(|row| row.id == "slowspots").unwrap();
        let frame_metrics = statuses
            .iter()
            .find(|row| row.id == "frame_metrics")
            .unwrap();
        let search = statuses
            .iter()
            .find(|row| row.id == "search_speed")
            .unwrap();

        assert_eq!(slowspots.status, "loaded");
        assert!(slowspots.available);
        assert_eq!(frame_metrics.status, "unparseable");
        assert!(!frame_metrics.available);
        assert!(frame_metrics.error.is_some());
        assert_eq!(search.status, "missing");
    }
}
