use super::stats::criterion_dir;
use crate::{provenance, shared};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

pub(super) fn suite_names<'a>(args: &'a [&'a str]) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == "--bench")
        .map(|pair| pair[1])
        .collect()
}

fn measurement_name(group: &str, parameter: &Value) -> String {
    format!(
        "{group}/{}",
        parameter
            .as_str()
            .map_or_else(|| parameter.to_string(), str::to_owned)
    )
}

fn expected_measurements(root: &Path, suites: &[&str]) -> Result<BTreeSet<String>> {
    let declaration: Value = serde_json::from_slice(
        &std::fs::read(root.join("benches/measurement_manifest.json"))
            .context("missing project-owned benches/measurement_manifest.json")?,
    )?;
    let mut expected = BTreeSet::new();
    for suite in suites {
        let groups = declaration["suites"][*suite]
            .as_object()
            .context("missing expected benchmark suite")?;
        ensure!(!groups.is_empty(), "empty expected benchmark suite {suite}");
        for (group, parameters) in groups {
            let parameters = parameters
                .as_array()
                .context("expected parameters must be an array")?;
            expected.extend(
                parameters
                    .iter()
                    .map(|parameter| measurement_name(group, parameter)),
            );
        }
    }
    Ok(expected)
}

fn validate_run(
    root: &Path,
    suites: &[&str],
    entries: &serde_json::Map<String, Value>,
) -> Result<()> {
    let mut expected = expected_measurements(root, suites)?;
    let observed_groups: HashSet<_> = entries
        .keys()
        .map(|name| shared::benchmark_key(name))
        .collect();
    let metadata = shared::load_benchmark_metadata(root);
    for (key, meta) in metadata.iter().filter(|(_, meta)| {
        meta["benchmark_suite"]
            .as_str()
            .is_some_and(|suite| suites.contains(&suite))
    }) {
        ensure!(
            observed_groups.contains(key.as_str()),
            "benchmark run omitted expected group {key}"
        );
        expected.extend(
            meta["expected_parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|parameter| measurement_name(key, parameter)),
        );
    }
    for name in expected {
        ensure!(
            entries.contains_key(&name),
            "benchmark run omitted expected measurement {name}"
        );
    }
    Ok(())
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
        validate_estimates(&serde_json::from_slice(&bytes)?)?;
        entries.insert(name, json!(provenance::fingerprint(&bytes)));
    }
    ensure!(
        !entries.is_empty(),
        "benchmark run produced no fresh Criterion estimates"
    );
    validate_run(root, suites, &entries)?;
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
    let directory = criterion_dir(root);
    let measured_at = suites
        .iter()
        .filter_map(|suite| {
            let shared::JsonRead::Loaded(manifest) =
                shared::read_json(&directory.join(format!("splens-{suite}.json")))
            else {
                return None;
            };
            let finished = manifest["finished_at"].as_f64()?;
            (manifest["context"] == current
                && (0.0..=86400.0).contains(&(provenance::timestamp() - finished))
                && manifest["entries"][name] == provenance::fingerprint(&bytes))
            .then_some(finished)
        })
        .max_by(f64::total_cmp)
        .context(format!("unverified or stale Criterion measurement: {name}"))?;
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
    let bounds = interval["lower_bound"]
        .as_f64()
        .zip(interval["upper_bound"].as_f64())
        .filter(|(lo, hi)| {
            estimates["mean"]["point_estimate"]
                .as_f64()
                .is_some_and(|mean| *lo > 0.0 && *lo <= mean && mean <= *hi)
                && interval["confidence_level"]
                    .as_f64()
                    .is_some_and(|level| level > 0.0 && level < 1.0)
        });
    row["mean_confidence_interval_ns"] = bounds.map_or(Value::Null, |_| interval.clone());
    row["budget_probe_ns"] = json!(bounds.map(|(_, hi)| hi));
    row["budget_probe_label"] = json!("mean_confidence_interval_upper_bound");
    row["tail_latency_measured"] = json!(false);
    row["budget_status"] = json!(match (bounds, threshold.filter(|value| *value > 0.0)) {
        (Some((lo, _)), Some(threshold)) if lo / 1e6 > threshold => "fail",
        (Some((_, hi)), Some(threshold)) if hi / 1e6 <= threshold => "pass",
        (Some(_), Some(_)) => "uncertain",
        _ => "unmeasured",
    });
}

#[cfg(test)]
mod tests {
    use super::{annotate_budget, read_estimates, record_run, validate_estimates};
    use crate::provenance;
    use serde_json::json;

    #[test]
    fn estimate_and_budget_boundaries_preserve_uncertainty() {
        let mut estimates = json!({"mean":{"point_estimate":100_000_000,
            "confidence_interval":{"lower_bound":90_000_000,"upper_bound":110_000_000,"confidence_level":0.95}},
            "median":{"point_estimate":100_000_000},"std_dev":{"point_estimate":0}});
        assert!(validate_estimates(&estimates).is_ok());
        for (budget, status) in [
            (Some(80.0), "fail"),
            (Some(100.0), "uncertain"),
            (Some(110.0), "pass"),
            (None, "unmeasured"),
            (Some(0.0), "unmeasured"),
        ] {
            let mut row = json!({});
            annotate_budget(&mut row, &estimates, budget);
            assert_eq!(row["budget_status"], status);
            assert_eq!(row["tail_latency_measured"], false);
        }
        for invalid in [json!(null), json!(0), json!(-1)] {
            estimates["mean"]["point_estimate"] = invalid;
            assert!(validate_estimates(&estimates).is_err());
            let mut row = json!({});
            annotate_budget(&mut row, &estimates, Some(100.0));
            assert_eq!(row["budget_status"], "unmeasured");
            assert!(row["budget_probe_ns"].is_null());
        }
    }

    #[test]
    fn run_manifests_require_complete_fresh_content_bound_measurements() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join("benches"))?;
        let declaration = root.path().join("benches/measurement_manifest.json");
        let directory = super::criterion_dir(root.path());
        let path = directory.join("custom/32/new/estimates.json");
        let suites = ["fixture"];
        let context = provenance::context(root.path());
        assert!(record_run(root.path(), &suites, 0.0, context.clone()).is_err());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, json!({"mean":{"point_estimate":10},"median":{"point_estimate":10},"std_dev":{"point_estimate":0}}).to_string())?;
        assert!(record_run(
            root.path(),
            &suites,
            provenance::timestamp() + 60.0,
            context.clone()
        )
        .is_err());
        for invalid in [
            json!({}),
            json!({"fixture":{}}),
            json!({"fixture":{"custom":null}}),
            json!({"fixture":{"custom":[64]}}),
        ] {
            std::fs::write(&declaration, json!({"suites":invalid}).to_string())?;
            assert!(record_run(root.path(), &suites, 0.0, context.clone()).is_err());
            assert!(!directory.join("splens-fixture.json").exists());
        }
        std::fs::write(
            &declaration,
            json!({"suites":{"fixture":{"custom":["32"]}}}).to_string(),
        )?;
        let targets = root.path().join("benches/benchmark_targets.json");
        for metadata in [
            json!({"absent":{"benchmark_suite":"fixture"}}),
            json!({"custom":{"benchmark_suite":"fixture","expected_parameters":[64]}}),
        ] {
            std::fs::write(&targets, metadata.to_string())?;
            assert!(record_run(root.path(), &suites, 0.0, context.clone()).is_err());
        }
        std::fs::write(&targets, "{}")?;
        record_run(root.path(), &suites, 0.0, context.clone())?;
        assert!(read_estimates(root.path(), &path, "custom/32", &suites)?
            ["measurement_finished_at"]
            .is_number());
        let manifest_path = directory.join("splens-fixture.json");
        let original = std::fs::read(&manifest_path)?;
        for (field, value) in [
            ("context", json!({})),
            ("finished_at", json!(0)),
            ("finished_at", json!(provenance::timestamp() + 60.0)),
            ("entries", json!({})),
        ] {
            let mut manifest: serde_json::Value = serde_json::from_slice(&original)?;
            manifest[field] = value;
            std::fs::write(&manifest_path, manifest.to_string())?;
            assert!(read_estimates(root.path(), &path, "custom/32", &suites).is_err());
        }
        std::fs::write(&manifest_path, original)?;
        let mut changed = std::fs::read(&path)?;
        changed.push(b' '); // Still valid JSON, but no longer the recorded content.
        std::fs::write(&path, changed)?;
        assert!(read_estimates(root.path(), &path, "custom/32", &suites).is_err());
        Ok(())
    }
}
