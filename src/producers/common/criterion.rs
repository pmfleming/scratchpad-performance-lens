use super::stats::criterion_dir;
use crate::{provenance, shared};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn suite_names<'a>(args: &'a [&'a str]) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == "--bench")
        .map(|pair| pair[1])
        .collect()
}

pub(super) fn record_run(root: &Path, suites: &[&str], started: f64, context: Value) -> Result<()> {
    let directory = criterion_dir(root);
    let mut entries = serde_json::Map::new();
    for path in shared::criterion_estimate_paths(&directory) {
        let modified = path
            .metadata()?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs_f64();
        if modified < started {
            continue;
        }
        let name = shared::benchmark_name_from_estimate_path(&directory, &path)
            .context("invalid Criterion path")?;
        let bytes = std::fs::read(&path)?;
        let estimates: Value = serde_json::from_slice(&bytes)?;
        validate_estimates(&estimates)?;
        entries.insert(name, json!(provenance::fingerprint(&bytes)));
    }
    if entries.is_empty() {
        bail!("benchmark run produced no fresh Criterion estimates");
    }
    // Expected benchmark groups come from project-owned declarations, not historical cache contents.
    let declaration: Value = serde_json::from_slice(
        &std::fs::read(root.join("benches/measurement_manifest.json"))
            .context("missing project-owned benches/measurement_manifest.json")?,
    )?;
    for suite in suites {
        let groups = declaration["suites"][*suite]
            .as_object()
            .context("missing expected benchmark suite")?;
        if groups.is_empty() {
            bail!("empty expected benchmark suite {suite}");
        }
        for (group, parameters) in groups {
            for parameter in parameters
                .as_array()
                .context("expected parameters must be an array")?
            {
                let name = format!(
                    "{group}/{}",
                    parameter
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| parameter.to_string())
                );
                if !entries.contains_key(&name) {
                    bail!("benchmark run omitted expected measurement {name}");
                }
            }
        }
    }
    let metadata = shared::load_benchmark_metadata(root);
    for (key, meta) in &metadata {
        if meta["benchmark_suite"]
            .as_str()
            .is_some_and(|suite| suites.contains(&suite))
            && !entries
                .keys()
                .any(|name| shared::benchmark_key(name) == key)
        {
            bail!("benchmark run omitted expected group {key}");
        }
        if meta["benchmark_suite"]
            .as_str()
            .is_some_and(|suite| suites.contains(&suite))
        {
            for parameter in meta["expected_parameters"].as_array().into_iter().flatten() {
                let name = format!(
                    "{key}/{}",
                    parameter
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| parameter.to_string())
                );
                if !entries.contains_key(&name) {
                    bail!("benchmark run omitted expected measurement {name}");
                }
            }
        }
    }
    let manifest =
        json!({"context": context, "finished_at": provenance::timestamp(), "entries": entries});
    for suite in suites {
        std::fs::write(
            directory.join(format!("splens-{suite}.json")),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
    }
    Ok(())
}

pub(in crate::producers) fn read_estimates(
    root: &Path,
    path: &Path,
    name: &str,
    suites: &[&str],
) -> Result<Value> {
    let bytes = std::fs::read(path)?;
    let mut estimates: Value = serde_json::from_slice(&bytes).context("invalid Criterion JSON")?;
    validate_estimates(&estimates)?;
    let current = provenance::context(root);
    let measured_at = suites
        .iter()
        .filter_map(|suite| {
            let shared::JsonRead::Loaded(manifest) =
                shared::read_json(&criterion_dir(root).join(format!("splens-{suite}.json")))
            else {
                return None;
            };
            let valid = manifest["context"] == current
                && manifest["finished_at"].as_f64().is_some_and(|time| {
                    (0.0..=86400.0).contains(&(provenance::timestamp() - time))
                })
                && manifest["entries"][name] == provenance::fingerprint(&bytes);
            valid.then(|| manifest["finished_at"].as_f64()).flatten()
        })
        .max_by(f64::total_cmp);
    if measured_at.is_none() {
        bail!("unverified or stale Criterion measurement: {name}");
    }
    estimates["measurement_finished_at"] = json!(measured_at);
    Ok(estimates)
}

fn validate_estimates(value: &Value) -> Result<()> {
    for key in ["mean", "median", "std_dev"] {
        let number = value[key]["point_estimate"]
            .as_f64()
            .context("missing numeric Criterion estimate")?;
        if !number.is_finite() || number < 0.0 || (key != "std_dev" && number == 0.0) {
            bail!("invalid {key} estimate");
        }
    }
    Ok(())
}

pub(in crate::producers) fn annotate_budget(
    row: &mut Value,
    estimates: &Value,
    threshold: Option<f64>,
) {
    let interval = &estimates["mean"]["confidence_interval"];
    let lower = interval["lower_bound"].as_f64();
    let upper = interval["upper_bound"].as_f64();
    let mean = estimates["mean"]["point_estimate"].as_f64();
    let level = interval["confidence_level"].as_f64();
    let valid = matches!((lower, mean, upper, level), (Some(lo), Some(mean), Some(hi), Some(level))
        if lo > 0.0 && lo <= mean && mean <= hi && level > 0.0 && level < 1.0);
    row["mean_confidence_interval_ns"] = if valid { interval.clone() } else { Value::Null };
    row["budget_probe_ns"] = if valid { json!(upper) } else { Value::Null };
    row["budget_probe_label"] = json!("mean_confidence_interval_upper_bound");
    row["tail_latency_measured"] = json!(false);
    row["budget_status"] = json!(match (valid, threshold.filter(|value| *value > 0.0)) {
        (true, Some(threshold)) if lower.unwrap() / 1e6 > threshold => "fail",
        (true, Some(threshold)) if upper.unwrap() / 1e6 <= threshold => "pass",
        (true, Some(_)) => "uncertain",
        _ => "unmeasured",
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confidence_interval_not_dispersion_controls_budget_and_overlap_is_uncertain() {
        let estimates = json!({"mean":{"point_estimate":100_000_000,
            "confidence_interval":{"lower_bound":90_000_000,"upper_bound":110_000_000,"confidence_level":0.95}}});
        for (budget, status) in [(80.0, "fail"), (100.0, "uncertain"), (120.0, "pass")] {
            let mut row = json!({});
            annotate_budget(&mut row, &estimates, Some(budget));
            assert_eq!(row["budget_status"], status);
            assert_eq!(row["tail_latency_measured"], false);
        }
        let mut row = json!({});
        annotate_budget(&mut row, &json!({}), Some(100.0));
        assert_eq!(row["budget_status"], "unmeasured");
        assert!(row["budget_probe_ns"].is_null());
    }
    #[test]
    fn missing_zero_and_negative_estimates_are_invalid() {
        assert!(validate_estimates(&json!({})).is_err());
        assert!(validate_estimates(&json!({"mean":{"point_estimate":0},"median":{"point_estimate":1},"std_dev":{"point_estimate":1}})).is_err());
        assert!(validate_estimates(&json!({"mean":{"point_estimate":1},"median":{"point_estimate":1},"std_dev":{"point_estimate":0}})).is_ok());
    }
}
