//! The application manifest owns budgets and the minimum production workload.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

pub(super) fn validate(payload: &Value, manifest: &Value) -> Result<()> {
    for row in payload["scenarios"]
        .as_array()
        .context("missing workflow rows")?
    {
        let key = row["benchmark_key"]
            .as_str()
            .context("missing workflow key")?;
        let mode = row["mode"].as_str().context("missing workflow mode")?;
        let operation = row["latency_kind"]
            .as_str()
            .context("missing workflow operation")?;
        ensure!(
            key == format!("search_workflow_{mode}_{operation}"),
            "workflow identity mismatch"
        );
        let budget = &manifest["probe_measurements"][key];
        ensure!(
            budget["probe"] == "search_workflow_metrics"
                && budget["threshold_ms"]
                    .as_f64()
                    .is_some_and(|v| v.is_finite() && v > 0.0),
            "{key}: missing project-owned workflow budget"
        );
        ensure!(
            row["threshold_ms"].as_f64() == budget["threshold_ms"].as_f64(),
            "{key}: probe budget differs from project manifest"
        );
        let workload = &manifest["workflow_workloads"][mode];
        ensure!(
            ["targets", "bytes_per_target"]
                .iter()
                .all(|k| workload[*k].as_u64().is_some_and(|n| n > 0)),
            "{key}: missing project-owned workload"
        );
        let targets = row["parameter_value"]
            .as_u64()
            .filter(|v| *v > 0)
            .context("invalid workflow target count")?;
        let bytes = row["bytes_per_target"]
            .as_u64()
            .filter(|v| *v > 0)
            .context("invalid workflow fixture bytes")?;
        let total = targets
            .checked_mul(bytes)
            .context("workflow byte count overflow")?;
        ensure!(
            row["total_bytes"] == total,
            "{key}: inconsistent workload bytes"
        );
        let raw = row["raw_samples"]
            .as_array()
            .context("missing raw workflow trials")?;
        let samples = row["sample_elapsed_ms"]
            .as_array()
            .context("missing workflow timings")?;
        ensure!(
            !raw.is_empty() && raw.len() == samples.len(),
            "{key}: incomplete raw workflow trials"
        );
        for (trial, timing) in raw.iter().zip(samples) {
            ensure!(
                trial["match_count"] == targets && trial["actual_total_bytes"] == total,
                "{key}: observed fixture or match count disagrees with workload"
            );
            let ns = trial[operation]
                .as_f64()
                .context("missing raw operation time")?;
            let ms = timing.as_f64().context("invalid workflow time")?;
            ensure!(
                close(ns / 1e6, ms),
                "{key}: raw and summarized trial times disagree"
            );
        }
        let mut values: Vec<_> = samples.iter().filter_map(Value::as_f64).collect();
        values.sort_by(f64::total_cmp);
        let median = (values[values.len() / 2] + values[(values.len() - 1) / 2]) / 2.0;
        for (field, expected) in [
            ("median_ms", median),
            ("max_ms", *values.last().unwrap()),
            ("mean_ms", values.iter().sum::<f64>() / values.len() as f64),
        ] {
            ensure!(
                row[field]
                    .as_f64()
                    .is_some_and(|value| close(value, expected)),
                "{key}: {field} disagrees with raw trials"
            );
        }
    }
    Ok(())
}

fn close(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-9 * a.abs().max(1.0)
}

pub(super) fn annotate(payload: &mut Value, manifest: &Value) {
    for row in payload["scenarios"].as_array_mut().into_iter().flatten() {
        let required = &manifest["workflow_workloads"][row["mode"].as_str().unwrap_or("")];
        let met = [
            ("parameter_value", "targets"),
            ("bytes_per_target", "bytes_per_target"),
        ]
        .iter()
        .all(|(actual, minimum)| {
            row[*actual]
                .as_u64()
                .zip(required[*minimum].as_u64())
                .is_some_and(|(a, b)| a >= b)
        });
        row["required_workload"] = required.clone();
        row["promise_workload_met"] = json!(met);
        row["threshold_source"] = json!("benches/measurement_manifest.json");
        if !met {
            row["measurement_status"] = json!("diagnostic");
            row["observed_budget_status"] = row["budget_status"].clone();
            row["budget_status"] = json!("unmeasured");
            row["measurement_note"] =
                json!("Reduced fixture: does not demonstrate the promised workload");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Value) {
        let manifest = json!({"workflow_workloads":{"all":{"targets":10000,"bytes_per_target":16384}},
            "probe_measurements":{"search_workflow_all_first_render":{"probe":"search_workflow_metrics","threshold_ms":120}}});
        let row = json!({"benchmark_key":"search_workflow_all_first_render","mode":"all","latency_kind":"first_render",
            "threshold_ms":120,"parameter_value":100,"bytes_per_target":16384,"total_bytes":1638400,
            "raw_samples":[{"match_count":100,"actual_total_bytes":1638400,"first_render":1000000}],
            "sample_elapsed_ms":[1.0],"median_ms":1.0,"max_ms":1.0,"mean_ms":1.0,"budget_status":"pass"});
        (json!({"scenarios":[row]}), manifest)
    }
    #[test]
    fn reduced_workload_cannot_claim_a_promise_even_with_passing_trials() {
        let (mut payload, manifest) = fixture();
        validate(&payload, &manifest).unwrap();
        annotate(&mut payload, &manifest);
        let row = &payload["scenarios"][0];
        assert_eq!(row["promise_workload_met"], false);
        assert_eq!(row["budget_status"], "unmeasured");
        assert_eq!(row["observed_budget_status"], "pass");
        assert_eq!(row["measurement_status"], "diagnostic");
    }
    #[test]
    fn mismatched_budgets_fixtures_and_trial_summaries_are_rejected() {
        let (original, manifest) = fixture();
        for (pointer, value) in [
            ("/threshold_ms", json!(1000)),
            ("/total_bytes", json!(1)),
            ("/raw_samples/0/match_count", json!(1)),
            ("/raw_samples/0/actual_total_bytes", json!(1)),
            ("/raw_samples/0/first_render", json!(100)),
            ("/raw_samples", json!([])),
            ("/median_ms", json!(0.1)),
            ("/max_ms", json!(0.1)),
            ("/mean_ms", json!(0.1)),
        ] {
            let mut payload = original.clone();
            *payload["scenarios"][0].pointer_mut(pointer).unwrap() = value;
            assert!(validate(&payload, &manifest).is_err(), "{pointer}");
        }
        assert!(validate(&original, &json!({})).is_err());
    }
    #[test]
    fn full_workload_preserves_observed_status() {
        let (mut payload, mut manifest) = fixture();
        manifest["workflow_workloads"]["all"]["targets"] = json!(100);
        payload["scenarios"][0]["measurement_status"] = json!("valid");
        validate(&payload, &manifest).unwrap();
        annotate(&mut payload, &manifest);
        assert_eq!(payload["scenarios"][0]["promise_workload_met"], true);
        assert_eq!(payload["scenarios"][0]["budget_status"], "pass");
        assert_eq!(payload["scenarios"][0]["measurement_status"], "valid");
    }
}
