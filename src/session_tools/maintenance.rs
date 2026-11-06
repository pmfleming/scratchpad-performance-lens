//! Maintenance regression measurements are diagnostics, not invented latency budgets.
use super::dirty_save::{measure_with, MeasureLargeDirtySaveArgs};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

pub(crate) fn measure_session_maintenance(args: MeasureLargeDirtySaveArgs) -> Result<()> {
    let manifest = args.project_manifest()?;
    let contract = manifest["diagnostics"]["session_maintenance"].clone();
    ensure!(
        contract["scenarios"]
            .as_array()
            .is_some_and(|scenarios| !scenarios.is_empty()),
        "missing maintenance scenario declaration"
    );
    measure_with(
        &args,
        Duration::from_secs(600),
        "--session-maintenance",
        &contract,
        |text, launch| summarize(text, launch, &contract),
    )
}

fn summarize(text: &str, launch: u8, contract: &Value) -> Result<Value> {
    let scenarios = contract["scenarios"]
        .as_array()
        .context("missing maintenance scenarios")?;
    let count = contract["samples_per_scenario"]
        .as_u64()
        .filter(|n| *n >= 3 && *n <= 1000)
        .context("invalid maintenance sample count")? as usize;
    let mut ids = std::collections::HashSet::new();
    for scenario in scenarios {
        let id = scenario["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("invalid maintenance scenario ID")?;
        ensure!(ids.insert(id), "duplicate maintenance scenario {id}");
        ensure!(
            scenario["save_required"].is_boolean(),
            "missing save requirement for {id}"
        );
    }
    let samples = text
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        samples.len() == scenarios.len() * count,
        "wrong sample count for declared maintenance scenarios"
    );
    let mut summaries = Vec::new();
    for declaration in scenarios {
        let scenario = declaration["id"].as_str().unwrap();
        let rows: Vec<_> = samples
            .iter()
            .filter(|row| row["scenario"] == scenario)
            .collect();
        ensure!(rows.len() == count, "missing or extra {scenario} samples");
        let mut seen = vec![false; count];
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
                crate::fixture_contract::matches(&row["workload_details"], &contract["fixture"]),
                "wrong maintenance fixture"
            );
            let step = row["step_index"].as_u64().context("missing step")?;
            ensure!(
                step < count as u64 && !seen[step as usize],
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
            let is_save = declaration["save_required"] == true;
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
            "scenario": scenario, "median_action_ms": action[count/2], "median_save_ms": save[count/2],
            "median_elapsed_ms": elapsed[count/2], "max_elapsed_ms": elapsed[count-1],
            "median_snapshot_bytes_hashed": hashed[count/2], "max_snapshot_bytes_hashed": hashed[count-1],
            "median_write_bytes": if writes.len() == count { Some(writes[count/2]) } else { None },
            "write_bytes_available": writes.len() == count,
        }));
    }
    Ok(json!({"launch": launch, "scenarios": summaries}))
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use serde_json::{json, Value};
    const SCENARIOS: &[&str] = &[
        "stored_usage",
        "closed_token_lookup",
        "closed_archive_append",
        "save_after_branch",
        "save_after_navigation",
        "incremental_save",
    ];
    fn contract() -> Value {
        json!({"samples_per_scenario":9,"fixture":{"equals":{"fixture_revision":1,"buffer_bytes":52428800,"initial_archives":128,"synchronous":"FULL"}},
        "scenarios": SCENARIOS.iter().map(|id|json!({"id":id,"save_required":id.starts_with("save_after_") || *id == "incremental_save"})).collect::<Vec<_>>()})
    }
    fn summarize(text: &str, launch: u8) -> Result<Value> {
        super::summarize(text, launch, &contract())
    }

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
    fn renamed_scenarios_follow_the_project_contract() {
        let mut declaration = contract();
        declaration["scenarios"][3]["id"] = json!("future_history_operation");
        let mut rows = samples();
        for row in &mut rows {
            if row["scenario"] == "save_after_branch" {
                row["scenario"] = json!("future_history_operation");
            }
        }
        assert!(super::summarize(&lines(&rows), 0, &declaration).is_ok());
        assert!(super::summarize(&lines(&rows), 0, &contract()).is_err());
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
