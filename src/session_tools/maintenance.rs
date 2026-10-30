//! Maintenance regression measurements are diagnostics, not invented latency budgets.
use super::dirty_save::{measure_with, MeasureLargeDirtySaveArgs};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

const SCENARIOS: &[&str] = &[
    "stored_usage",
    "closed_token_lookup",
    "closed_archive_append",
    "save_after_history_backfill",
    "save_after_navigation",
    "periodic_snapshot_save",
];

pub(crate) fn measure_session_maintenance(args: MeasureLargeDirtySaveArgs) -> Result<()> {
    measure_with(
        &args,
        Duration::from_secs(600),
        "--session-maintenance",
        summarize,
    )
}

fn summarize(text: &str, launch: u8) -> Result<Value> {
    let samples = text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        samples.len() == SCENARIOS.len() * 9,
        "expected nine samples for each maintenance scenario"
    );
    let mut summaries = Vec::new();
    for scenario in SCENARIOS {
        let rows: Vec<_> = samples
            .iter()
            .filter(|row| row["scenario"] == *scenario)
            .collect();
        ensure!(rows.len() == 9, "missing or extra {scenario} samples");
        let mut seen = [false; 9];
        let mut action = Vec::new();
        let mut save = Vec::new();
        let mut elapsed = Vec::new();
        let mut writes = Vec::new();
        let mut hashed = Vec::new();
        for row in rows {
            ensure!(row["status"] == "ok", "failed {scenario} sample");
            ensure!(
                row["measurement_scope"] == "prepared_operation",
                "fixture work included in {scenario}"
            );
            ensure!(
                row["workload_details"]["fixture_revision"] == 1
                    && row["workload_details"]["buffer_bytes"] == 50 * 1024 * 1024
                    && row["workload_details"]["initial_archives"] == 128
                    && row["workload_details"]["synchronous"] == "FULL",
                "wrong maintenance fixture"
            );
            let step = row["step_index"].as_u64().context("missing step")?;
            ensure!(
                step < 9 && !seen[step as usize],
                "duplicate or invalid step"
            );
            seen[step as usize] = true;
            let duration = |value: &Value| -> Result<f64> {
                let ns = value.as_f64().context("missing duration")?;
                ensure!(ns.is_finite() && ns >= 0.0, "invalid duration");
                Ok(ns / 1e6)
            };
            action.push(duration(&row["phase_elapsed_ns"]["action_ns"])?);
            let save_ms = duration(&row["phase_elapsed_ns"]["save_ns"])?;
            let is_save =
                scenario.starts_with("save_after_") || *scenario == "periodic_snapshot_save";
            ensure!(
                !is_save || save_ms > 0.0,
                "missing subsequent save duration"
            );
            save.push(save_ms);
            let total = duration(&row["elapsed_ns"])?;
            ensure!(total > 0.0, "missing operation duration");
            elapsed.push(total);
            ensure!(
                row["result_unit"] == "snapshot-bytes-hashed",
                "missing byte-work metric"
            );
            hashed.push(
                row["result_value"]
                    .as_u64()
                    .context("invalid bytes hashed")?,
            );
            let bytes = &row["process_resources"]["write_bytes"];
            if !bytes.is_null() {
                writes.push(bytes.as_u64().context("invalid write bytes")?);
            }
        }
        action.sort_by(f64::total_cmp);
        save.sort_by(f64::total_cmp);
        elapsed.sort_by(f64::total_cmp);
        writes.sort_unstable();
        hashed.sort_unstable();
        summaries.push(json!({
            "scenario": scenario, "median_action_ms": action[4], "median_save_ms": save[4],
            "median_elapsed_ms": elapsed[4], "max_elapsed_ms": elapsed[8],
            "median_snapshot_bytes_hashed": hashed[4], "max_snapshot_bytes_hashed": hashed[8],
            "median_write_bytes": if writes.len() == 9 { Some(writes[4]) } else { None },
            "write_bytes_available": writes.len() == 9,
        }));
    }
    Ok(json!({"launch": launch, "scenarios": summaries}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn samples() -> Vec<Value> {
        SCENARIOS.iter().flat_map(|scenario| (0..9).map(move |step| json!({
            "scenario": scenario, "step_index": step, "status": "ok", "measurement_scope": "prepared_operation",
            "workload_details": {"fixture_revision": 1, "buffer_bytes": 50*1024*1024, "initial_archives": 128, "synchronous": "FULL"},
            "phase_elapsed_ns": {"action_ns": 1_000_000, "save_ns": 2_000_000}, "elapsed_ns": 3_000_000,
            "result_unit": "snapshot-bytes-hashed", "result_value": 65536,
            "process_resources": {"write_bytes": null}
        }))).collect()
    }
    fn lines(rows: &[Value]) -> String {
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn summarizes_each_scenario_without_inventing_io_or_budgets() {
        let result = summarize(&lines(&samples()), 2).unwrap();
        assert_eq!(result["launch"], 2);
        assert_eq!(
            result["scenarios"].as_array().unwrap().len(),
            SCENARIOS.len()
        );
        for row in result["scenarios"].as_array().unwrap() {
            assert_eq!(row["median_elapsed_ms"], 3.0);
            assert_eq!(row["median_snapshot_bytes_hashed"], 65536);
            assert!(row["median_write_bytes"].is_null());
        }
    }
    #[test]
    fn rejects_incomplete_failed_duplicate_wrong_fixture_and_missing_metrics() {
        for (field, value) in [
            ("status", json!("panic")),
            ("scenario", json!("other")),
            ("step_index", json!(1)),
            ("measurement_scope", json!("setup")),
            ("workload_details", json!({})),
            ("result_value", json!(-1)),
            ("elapsed_ns", Value::Null),
            ("phase_elapsed_ns", json!({"action_ns": 1})),
        ] {
            let mut rows = samples();
            rows[0][field] = value;
            assert!(summarize(&lines(&rows), 0).is_err(), "{field}");
        }
        let mut rows = samples();
        rows.pop();
        assert!(summarize(&lines(&rows), 0).is_err());
        assert!(summarize("invalid", 0).is_err());
    }
}
