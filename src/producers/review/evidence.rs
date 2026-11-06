use super::super::common::{budget_probe_ms, values_array};
use super::registry::ReviewScenario;
use crate::artifacts::performance_review::SourceArtifactStatus;
use crate::config::LensConfig;
use crate::shared;
use serde_json::Value;
use std::collections::BTreeMap;
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
    scenario.benchmark_keys.contains(&key)
        || scenario.families.contains(&family)
        || row["promise_scenarios"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id == scenario.id))
}

pub(super) fn over_budget_latency(row: &&Value) -> bool {
    if row["measurement_status"]
        .as_str()
        .is_some_and(|status| status != "valid")
    {
        return false;
    }
    if row["evidence_kind"] == "frame" {
        return super::super::common::frame_over_budget(row);
    }
    if let Some(status) = row["budget_status"].as_str() {
        return status == "fail";
    }
    let budget_probe = budget_probe_ms(row);
    let threshold = row
        .get("threshold_ms")
        .or_else(|| row.get("budget_ms"))
        .and_then(Value::as_f64);
    matches!((budget_probe, threshold), (Some(value), Some(threshold)) if threshold > 0.0 && value > threshold)
}

pub(super) fn resource_contract_failed(row: &Value) -> bool {
    row["budget_status"] == "fail"
        || row["cache_bound_held"] == false
        || row["cache_bound_violations"]
            .as_u64()
            .is_some_and(|count| count > 0)
        || values_array(&row["samples"]).iter().any(|sample| {
            sample["fixture_valid"] == false
                || sample["status"]
                    .as_str()
                    .is_some_and(|status| status != "ok")
        })
}

pub(super) fn unique_rows(rows: Vec<Value>) -> Vec<Value> {
    let mut unique: BTreeMap<String, Value> = BTreeMap::new();
    for row in rows {
        let name = row["name"]
            .as_str()
            .or_else(|| row["scenario_id"].as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{}/{}",
                    row["benchmark_key"].as_str().unwrap_or("unknown"),
                    row["parameter_value"]
                )
            });
        let kind = if row["evidence_kind"] == "frame" {
            "frame"
        } else if row["budget_probe_label"] == "median_of_independent_trials" {
            "workflow"
        } else {
            "criterion"
        };
        // Criterion names already encode the workload. Report-specific labels,
        // inferred parameter fields and descriptions are not measurement identity.
        let scope = if kind == "criterion" {
            ""
        } else {
            row["measurement_scope"].as_str().unwrap_or("")
        };
        let key = format!("{kind}|{name}|{scope}");
        let quality = |value: &Value| {
            (
                value["measurement_status"] == "valid",
                value["measurement_finished_at"]
                    .as_f64()
                    .map(|time| (time * 1_000.0) as u64)
                    .unwrap_or(0),
                value["threshold_authoritative"] == true,
                value.as_object().map_or(0, |object| object.len()),
            )
        };
        if unique
            .get(&key)
            .is_none_or(|old| quality(&row) > quality(old))
        {
            unique.insert(key, row);
        }
    }
    unique.into_values().collect()
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

pub(super) fn source_status(config: &LensConfig, context: &Value) -> Vec<SourceArtifactStatus> {
    [
        ("slowspots", "slowspots.json"),
        ("frame_metrics", "frame_metrics.json"),
        ("search_speed", "search_speed.json"),
        ("search_workflows", "search_workflow_metrics.json"),
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
                let validity = crate::provenance::validity(&path, context);
                (
                    validity == "valid",
                    if validity == "valid" {
                        "loaded"
                    } else {
                        validity
                    },
                    probe_status,
                    None,
                )
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
    use super::{resource_contract_failed, source_status};
    use crate::config::LensConfig;

    #[test]
    fn duplicate_criterion_rows_ignore_presentation_but_preserve_frame_scope() {
        let rows = super::unique_rows(vec![
            serde_json::json!({"name":"search/32", "benchmark_key":"search", "measurement_status":"valid"}),
            serde_json::json!({"name":"search/32", "benchmark_key":"search", "parameter_value":32,
                "scenario_label":"Search", "measurement_status":"valid", "threshold_authoritative":true}),
            serde_json::json!({"name":"search/32", "evidence_kind":"frame", "measurement_scope":"event_to_tessellation"}),
        ]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["threshold_authoritative"], true);
        assert_eq!(rows[1]["evidence_kind"], "frame");
    }

    #[test]
    fn resource_contract_detects_cache_violations_and_failed_samples() {
        for (held, violations, status, failed) in [
            (false, 0, "ok", true),
            (true, 1, "ok", true),
            (true, 0, "panic", true),
            (true, 0, "ok", false),
        ] {
            assert_eq!(
                resource_contract_failed(&serde_json::json!({
                    "cache_bound_held":held, "cache_bound_violations":violations, "samples":[{"status":status}]
                })),
                failed
            );
        }
    }

    #[test]
    fn source_status_distinguishes_loaded_missing_and_unparseable() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("slowspots.json"), "[]")?;
        std::fs::write(dir.path().join("frame_metrics.json"), "{truncated")?;
        let config = LensConfig {
            project_name: "fixture".to_string(),
            project_root: dir.path().to_path_buf(),
            output_dir: dir.path().to_path_buf(),
        };

        let statuses = source_status(&config, &crate::provenance::context(&config.project_root));
        for (id, status, has_error) in [
            ("slowspots", "unverified", false),
            ("frame_metrics", "unparseable", true),
            ("search_speed", "missing", false),
        ] {
            let row = statuses.iter().find(|row| row.id == id).unwrap();
            assert_eq!(row.status, status);
            assert!(!row.available);
            assert_eq!(row.error.is_some(), has_error);
        }
        Ok(())
    }
}
