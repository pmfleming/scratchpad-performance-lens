use super::stats::over_threshold;
use crate::{provenance, shared};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(in crate::producers) fn run_benchmarks(
    project_root: &Path,
    skip: bool,
    cargo_args: &[&str],
    label: &str,
) -> Option<String> {
    if !project_root.is_dir() {
        let root = project_root.display();
        return Some(format!("project root does not exist: {root}"));
    }
    if skip {
        return None;
    }
    let started = provenance::timestamp();
    let suites = suite_names(cargo_args);
    // Invalidate prior run manifests before executing: partial/failing reruns cannot reuse them.
    let directory = criterion_dir(project_root);
    for suite in &suites {
        let _ = std::fs::remove_file(directory.join(format!("splens-{suite}.json")));
    }
    shared::run_progress_command(project_root, cargo_args, label)
        .and_then(|()| {
            repeat_borderline(project_root, &suites, started)?;
            record_run(
                project_root,
                &suites,
                started,
                provenance::context(project_root),
            )
        })
        .err()
        .map(|error| error.to_string())
}

fn repeat_borderline(root: &Path, suites: &[&str], started: f64) -> Result<()> {
    let metadata = shared::load_benchmark_metadata(root);
    let manifest: Value = std::fs::read(root.join("benches/measurement_manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let directory = criterion_dir(root);
    for path in shared::criterion_estimate_paths(&directory) {
        let modified = path
            .metadata()?
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs_f64();
        if modified < started {
            continue;
        }
        let Some(name) = shared::benchmark_name_from_estimate_path(&directory, &path) else {
            continue;
        };
        let Some(limit) = metadata
            .get(shared::benchmark_key(&name))
            .and_then(shared::authoritative_threshold_ms)
            .filter(|v| *v > 0.0)
        else {
            continue;
        };
        let estimates: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        let near = estimates["mean"]["point_estimate"]
            .as_f64()
            .is_some_and(|ns| (0.9..=1.1).contains(&(ns / 1e6 / limit)));
        if !near {
            continue;
        }
        let Some(suite) = suites
            .iter()
            .find(|suite| manifest["suites"][**suite][shared::benchmark_key(&name)].is_array())
        else {
            continue;
        };
        let archive = directory.join(format!(
            "splens-near-budget-{:.9}-{}.json",
            started,
            crate::provenance::fingerprint(name.as_bytes())
        ));
        std::fs::write(
            &archive,
            serde_json::to_vec_pretty(&json!({"name":name,"first_estimates":estimates}))?,
        )?;
        shared::run_progress_command(
            root,
            &[
                "cargo",
                "bench",
                "--bench",
                suite,
                "--",
                &name,
                "--sample-size",
                "50",
            ],
            "Repeating borderline benchmark with 50 samples",
        )?;
    }
    Ok(())
}

fn criterion_dir(project_root: &Path) -> PathBuf {
    shared::cargo_target_directory(project_root).join("criterion")
}

pub(in crate::producers) fn collect_benchmarks(
    root: &Path,
    suites: &[&str],
    include: impl Fn(&Value) -> bool,
    build: fn(&str, &str, &Value, &Value) -> Value,
) -> (Vec<Value>, Vec<String>) {
    let metadata = shared::load_benchmark_metadata(root);
    let directory = criterion_dir(root);
    let manifests = load_manifests(&directory, suites, &provenance::context(root));
    if manifests.is_empty() || manifests.len() != suites.len() {
        return (
            Vec::new(),
            vec![
                "no current verified Criterion run manifest for one or more requested suites"
                    .into(),
            ],
        );
    }
    let paths: HashMap<_, _> = shared::criterion_estimate_paths(&directory)
        .into_iter()
        .filter_map(|path| {
            Some((
                shared::benchmark_name_from_estimate_path(&directory, &path)?,
                path,
            ))
        })
        .collect();
    let names: BTreeSet<_> = manifests
        .iter()
        .filter_map(|manifest| manifest["entries"].as_object())
        .flat_map(|entries| entries.keys())
        .collect();
    let mut rows = Vec::new();
    let mut invalid = Vec::new();
    // The manifest is the inventory, not old Criterion directories. Missing or corrupt
    // listed entries still fail; unlisted leftovers cannot poison a complete new run.
    for name in names {
        let key = shared::benchmark_key(name);
        let Some(meta) = metadata.get(key).filter(|meta| include(meta)) else {
            continue;
        };
        let estimates = paths
            .get(name)
            .context("manifest-listed Criterion measurement is missing")
            .and_then(|path| read_estimates(path, name, &manifests));
        let row = match estimates {
            Ok(estimates) => build(name, key, meta, &estimates),
            Err(error) => {
                invalid.push(format!("{name}: {error}"));
                json!({"name": name, "benchmark_key": key,
                    "workload_family": meta["workload_family"], "measurement_status": "invalid",
                    "error": error.to_string(), "mean_ns": null, "budget_probe_ns": null})
            }
        };
        rows.push(row);
    }
    (rows, invalid)
}

// Validate after publication so failed runs still expose their incomplete evidence.
pub(in crate::producers) fn validate_measurements(
    payload: &Value,
    invalid: &[String],
    probe_failure: Option<String>,
    fail_on_slow: bool,
    label: &str,
) -> Result<()> {
    if let Some(error) = probe_failure {
        bail!("{label} probe failed: {error}");
    }
    ensure!(
        payload.as_array().is_some_and(|rows| !rows.is_empty()) && invalid.is_empty(),
        "{label} measurements incomplete or invalid: {}",
        invalid.join("; ")
    );
    ensure!(
        !fail_on_slow || !super::values_array(payload).iter().any(over_threshold),
        "one or more {label} benchmarks exceeded threshold"
    );
    Ok(())
}

pub(in crate::producers) fn annotate_measurement(row: &mut Value, estimates: &Value, meta: &Value) {
    let threshold = shared::authoritative_threshold_ms(meta);
    annotate_budget(row, estimates, threshold);
    row["measurement_finished_at"] = json!(estimates["measurement_finished_at"]);
    row["measurement_contract"] = meta.get("measurement_contract").cloned().unwrap_or_else(
        || json!({"text_backing":"unknown","production_path":false,"code_path":"undeclared"}),
    );
    row["threshold_authoritative"] = json!(threshold.is_some());
    row["threshold_source"] = json!(meta["threshold_source"]);
    row["metadata_source"] = json!(meta["metadata_source"]);
    row["stale_budget_risk"] = json!(meta["stale_budget_risk"].as_bool().unwrap_or(false));
}

fn suite_names<'a>(args: &'a [&'a str]) -> Vec<&'a str> {
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

fn record_run(root: &Path, suites: &[&str], started: f64, context: Value) -> Result<()> {
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

fn load_manifests(directory: &Path, suites: &[&str], current: &Value) -> Vec<Value> {
    let now = provenance::timestamp();
    suites
        .iter()
        .filter_map(|suite| {
            let shared::JsonRead::Loaded(manifest) =
                shared::read_json(&directory.join(format!("splens-{suite}.json")))
            else {
                return None;
            };
            let finished = manifest["finished_at"].as_f64()?;
            (manifest["context"] == *current && (0.0..=86400.0).contains(&(now - finished)))
                .then_some(manifest)
        })
        .collect()
}

fn read_estimates(path: &Path, name: &str, manifests: &[Value]) -> Result<Value> {
    let bytes = std::fs::read(path)?;
    let mut estimates: Value = serde_json::from_slice(&bytes).context("invalid Criterion JSON")?;
    validate_estimates(&estimates)?;
    let fingerprint = provenance::fingerprint(&bytes);
    let measured_at = manifests
        .iter()
        .filter_map(|manifest| {
            (manifest["entries"][name] == fingerprint)
                .then(|| manifest["finished_at"].as_f64())
                .flatten()
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

fn annotate_budget(row: &mut Value, estimates: &Value, threshold: Option<f64>) {
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
    use super::{annotate_budget, record_run, validate_estimates};
    use crate::provenance;
    use serde_json::json;

    fn read_estimates(
        root: &std::path::Path,
        path: &std::path::Path,
        name: &str,
        suites: &[&str],
    ) -> anyhow::Result<serde_json::Value> {
        let manifests = super::load_manifests(
            &super::criterion_dir(root),
            suites,
            &provenance::context(root),
        );
        super::read_estimates(path, name, &manifests)
    }

    #[cfg(unix)]
    #[test]
    fn benchmark_manifest_uses_post_command_context() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        assert!(crate::shared::run_command(root.path(), &["git", "init"])?
            .status
            .success());
        std::fs::write(root.path().join(".gitignore"), "target/\n")?;
        std::fs::write(root.path().join("Cargo.lock"), "before")?;
        std::fs::create_dir(root.path().join("benches"))?;
        std::fs::write(
            root.path().join("benches/measurement_manifest.json"),
            json!({"suites":{"fixture":{"custom":[32]}}}).to_string(),
        )?;
        let directory = super::criterion_dir(root.path());
        let path = directory.join("custom/32/new/estimates.json");
        std::fs::create_dir_all(path.parent().unwrap())?;
        let script = format!("printf after > Cargo.lock; printf '%s' '{{\"mean\":{{\"point_estimate\":10}},\"median\":{{\"point_estimate\":10}},\"std_dev\":{{\"point_estimate\":0}}}}' > '{}'", path.display());
        let before = provenance::context(root.path());
        let failure = provenance::with_run_snapshot(root.path(), || {
            super::run_benchmarks(
                root.path(),
                false,
                &["sh", "-c", &script, "--bench", "fixture"],
                "fixture benchmark",
            )
        });
        assert_eq!(failure, None);
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("splens-fixture.json"))?)?;
        assert_ne!(manifest["context"], before);
        assert_eq!(manifest["context"], provenance::context(root.path()));
        assert!(read_estimates(root.path(), &path, "custom/32", &["fixture"]).is_ok());
        Ok(())
    }

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
        std::fs::write(&path, json!({"mean":{"point_estimate":10,"confidence_interval":{"lower_bound":9,"upper_bound":11,"confidence_level":0.95}},"median":{"point_estimate":10},"std_dev":{"point_estimate":0}}).to_string())?;
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
        std::fs::write(
            &targets,
            json!({"custom":{"workload_family":"search","threshold_ms":0.000008}}).to_string(),
        )?;
        record_run(root.path(), &suites, 0.0, context.clone())?;
        assert!(read_estimates(root.path(), &path, "custom/32", &suites)?
            ["measurement_finished_at"]
            .is_number());
        let manifest_path = directory.join("splens-fixture.json");
        let original = std::fs::read(&manifest_path)?;
        // A removed/renamed parameter must not poison the new run; listed files must.
        let obsolete = directory.join("custom/64/new");
        std::fs::create_dir_all(&obsolete)?;
        std::fs::write(obsolete.join("estimates.json"), "{obsolete corrupt output")?;
        let collect = || {
            super::collect_benchmarks(
                root.path(),
                &suites,
                |_| true,
                |name, _, _, _| json!({"name":name}),
            )
        };
        let (rows, errors) = collect();
        assert_eq!(rows, vec![json!({"name":"custom/32"})]);
        assert!(errors.is_empty());
        let measured = std::fs::read(&path)?;
        std::fs::remove_file(&path)?;
        let (rows, errors) = collect();
        assert_eq!(rows[0]["measurement_status"], "invalid");
        assert_eq!(errors.len(), 1);
        std::fs::write(&path, measured)?;
        let (_, missing_suite) = super::collect_benchmarks(
            root.path(),
            &["fixture", "absent"],
            |_| true,
            |_, _, _, _| json!({}),
        );
        assert_eq!(missing_suite.len(), 1);
        for suite in ["search_speed", "frame_budget", "promise_latency"] {
            std::fs::copy(
                &manifest_path,
                directory.join(format!("splens-{suite}.json")),
            )?;
        }
        let config = crate::config::LensConfig {
            project_name: "fixture".into(),
            project_root: root.path().into(),
            output_dir: root.path().join("analysis"),
        };
        for fail_on_slow in [false, true] {
            let options = crate::cli::MeasureOptions {
                skip_bench: true,
                fail_on_slow,
                index_only: false,
            };
            assert_eq!(
                crate::producers::search_speed(&config, options).is_err(),
                fail_on_slow
            );
            assert_eq!(
                crate::producers::slowspots(&config, options).is_err(),
                fail_on_slow
            );
        }
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
        assert_eq!(collect().1.len(), 1);
        std::fs::write(&path, "{truncated")?;
        assert_eq!(collect().0[0]["measurement_status"], "invalid");
        Ok(())
    }
}
