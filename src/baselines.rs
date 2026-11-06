//! Accepted, content-bound snapshots spanning every promise and evidence axis.
use crate::{artifacts::performance_review::*, config::LensConfig, provenance};
use anyhow::{ensure, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Args)]
pub struct BaselineArgs {
    #[arg(long)]
    config: Option<PathBuf>,
    /// Accepted snapshot; defaults to OUTPUT/baselines/accepted.json.
    #[arg(long)]
    baseline: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Observation {
    median: f64,
    low: f64,
    high: f64,
}
#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    accepted_at: f64,
    environment: Value,
    source_revision: Value,
    observations: BTreeMap<String, Observation>,
    promises: BTreeMap<String, PromiseHealth>,
    content_fingerprint: String,
}

fn default_path(config: &LensConfig) -> PathBuf {
    config.output_dir.join("baselines/accepted.json")
}

pub fn environment(context: &Value) -> Value {
    let keys = [
        "host",
        "cpu",
        "os",
        "arch",
        "logical_cpus",
        "toolchain",
        "rustflags",
        "probe_environment",
        "measurement_environment",
    ];
    Value::Object(
        keys.into_iter()
            .map(|k| (k.into(), context[k].clone()))
            .collect(),
    )
}

fn fingerprint(snapshot: &Snapshot) -> String {
    provenance::fingerprint(&serde_json::to_vec(&json!({"environment":snapshot.environment,
        "revision":snapshot.source_revision,"observations":snapshot.observations,"promises":snapshot.promises})).unwrap())
}

fn read(path: &Path) -> Result<Snapshot> {
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        snapshot.version == 1 && snapshot.content_fingerprint == fingerprint(&snapshot),
        "invalid baseline fingerprint/version"
    );
    Ok(snapshot)
}

pub fn accept(args: BaselineArgs) -> Result<()> {
    let config = LensConfig::load(args.config.as_deref())?;
    let report = load_current(&config)?;
    ensure!(
        report.scenarios.len() == report.promise_count && report.promise_count > 0,
        "incomplete promise inventory"
    );
    ensure!(report.scenarios.iter().all(|s| s.coverage_status == CoverageStatus::Covered && !s.synthetic_evidence),
        "acceptance requires complete real evidence for every promise; budget failures may be accepted as baseline observations");
    let context = provenance::context(&config.project_root);
    let mut snapshot = snapshot(&context, &report.scenarios);
    ensure!(
        !snapshot.observations.is_empty(),
        "no valid baseline observations"
    );
    snapshot.content_fingerprint = fingerprint(&snapshot);
    let path = args.baseline.unwrap_or_else(|| default_path(&config));
    let parent = path.parent().context("baseline path has no parent")?;
    std::fs::create_dir_all(parent)?;
    // Keep prior accepted snapshots reviewable before changing the current pointer.
    if path.exists() {
        let previous = read(&path)?;
        let archive = parent.join(format!("accepted-{:.9}.json", previous.accepted_at));
        if !archive.exists() {
            std::fs::copy(&path, archive)?;
        }
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&snapshot)?)?;
    std::fs::rename(temporary, &path)?;
    println!("Accepted baseline: {}", path.display());
    Ok(())
}

pub fn compare(args: BaselineArgs) -> Result<()> {
    let config = LensConfig::load(args.config.as_deref())?;
    let report = load_current(&config)?;
    let baseline = read(&args.baseline.unwrap_or_else(|| default_path(&config)))?;
    let current = snapshot(
        &provenance::context(&config.project_root),
        &report.scenarios,
    );
    let comparisons: BTreeMap<_, _> = report
        .scenarios
        .iter()
        .map(|s| (s.id.clone(), comparison(&baseline, &current, &s.id)))
        .collect();
    let payload = serde_json::to_value(&comparisons)?;
    std::fs::write(
        config.output_dir.join("baseline_comparison.json"),
        serde_json::to_vec_pretty(&payload)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    ensure!(
        comparisons
            .values()
            .all(|c| c.status == RegressionStatus::Stable),
        "baseline comparison found regressions or incomparable evidence"
    );
    Ok(())
}

fn load_current(config: &LensConfig) -> Result<PerformanceReviewArtifact> {
    let path = config.output_dir.join("performance_review.json");
    ensure!(
        provenance::validity(&path, &provenance::context(&config.project_root)) == "valid",
        "review must have current verified provenance"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

pub(crate) fn annotate(
    config: &LensConfig,
    context: &Value,
    scenarios: &mut [PerformanceReviewScenario],
) -> Result<()> {
    let path = default_path(config);
    if !path.exists() {
        return Ok(());
    }
    let baseline = read(&path)?;
    let current = snapshot(context, scenarios);
    for scenario in scenarios {
        scenario.baseline_comparison = comparison(&baseline, &current, &scenario.id);
        if scenario.baseline_comparison.status == RegressionStatus::Regressed
            && scenario.promise_health == PromiseHealth::Pass
        {
            scenario.promise_health = PromiseHealth::PassingButRegressed;
        }
    }
    Ok(())
}

fn snapshot(context: &Value, scenarios: &[PerformanceReviewScenario]) -> Snapshot {
    let mut raw: BTreeMap<String, Vec<(f64, f64, f64)>> = BTreeMap::new();
    for scenario in scenarios {
        let mut add = |axis: &str,
                       id: &str,
                       workload: Value,
                       contract: &Value,
                       scope: &Value,
                       metric: &str,
                       value: Option<f64>,
                       low: Option<f64>,
                       high: Option<f64>| {
            if let Some(value) = value.filter(|v| v.is_finite() && *v >= 0.0) {
                let key = serde_json::to_string(&json!([
                    scenario.id,
                    axis,
                    id,
                    workload,
                    contract,
                    scope,
                    metric
                ]))
                .unwrap();
                raw.entry(key).or_default().push((
                    value,
                    low.unwrap_or(value),
                    high.unwrap_or(value),
                ));
            }
        };
        for row in &scenario.evidence.latency {
            if row["measurement_status"] != "valid" {
                continue;
            }
            let id = row["benchmark_key"].as_str().unwrap_or("");
            if !crate::measurement_contract::eligible(id, &row["measurement_contract"]) {
                continue;
            }
            for metric in if row["evidence_kind"] == "frame" {
                &["p95_ms", "p99_ms"][..]
            } else {
                &["budget_probe_ns"][..]
            } {
                add(
                    "latency",
                    id,
                    row["parameter_value"].clone(),
                    &row["measurement_contract"],
                    &row["measurement_scope"],
                    metric,
                    row[*metric].as_f64(),
                    None,
                    None,
                );
            }
        }
        for row in &scenario.evidence.capacity {
            let row: crate::artifacts::capacity_report::CapacityScenario =
                serde_json::from_value(row.clone())
                    .expect("review capacity was typed before serialization");
            for sample in &row.samples {
                if sample.status != "ok"
                    || !sample.complete
                    || !crate::measurement_contract::eligible(
                        &row.scenario,
                        &sample.measurement_contract,
                    )
                {
                    continue;
                }
                add(
                    "capacity",
                    &row.scenario,
                    json!(sample.workload_value),
                    &sample.measurement_contract,
                    &json!(sample.measurement_scope),
                    "elapsed_ms",
                    sample.elapsed_ms,
                    sample.elapsed_min_ms,
                    sample.elapsed_max_ms,
                );
            }
        }
        for row in &scenario.evidence.resources {
            let id = row["scenario"].as_str().unwrap_or("");
            for sample in row["samples"].as_array().into_iter().flatten() {
                if sample["status"] != "ok"
                    || !crate::measurement_contract::eligible(id, &sample["measurement_contract"])
                {
                    continue;
                }
                let metric = sample["resource_budget"]["metric"]
                    .as_str()
                    .unwrap_or("/elapsed_ns");
                add(
                    "resource",
                    id,
                    sample["workload_value"].clone(),
                    &sample["measurement_contract"],
                    &sample["measurement_scope"],
                    metric,
                    sample.pointer(metric).and_then(Value::as_f64),
                    None,
                    None,
                );
            }
        }
    }
    let observations = raw
        .into_iter()
        .map(|(key, mut values)| {
            values.sort_by(|a, b| a.0.total_cmp(&b.0));
            let value = &values[values.len() / 2];
            (
                key,
                Observation {
                    median: value.0,
                    low: values.iter().map(|v| v.1).fold(f64::INFINITY, f64::min),
                    high: values.iter().map(|v| v.2).fold(0.0, f64::max),
                },
            )
        })
        .collect();
    Snapshot {
        version: 1,
        accepted_at: provenance::timestamp(),
        environment: environment(context),
        source_revision: context["revision"].clone(),
        observations,
        promises: scenarios
            .iter()
            .map(|s| (s.id.clone(), s.promise_health))
            .collect(),
        content_fingerprint: String::new(),
    }
}

fn comparison(before: &Snapshot, after: &Snapshot, promise: &str) -> BaselineComparison {
    if before.environment != after.environment {
        return BaselineComparison {
            status: RegressionStatus::EnvironmentMismatch,
            ..Default::default()
        };
    }
    let prefix = format!("[{}", serde_json::to_string(promise).unwrap());
    let old: BTreeMap<_, _> = before
        .observations
        .iter()
        .filter(|(id, _)| id.starts_with(&prefix))
        .collect();
    let new: BTreeMap<_, _> = after
        .observations
        .iter()
        .filter(|(id, _)| id.starts_with(&prefix))
        .collect();
    let mut result = BaselineComparison {
        status: RegressionStatus::Stable,
        ..Default::default()
    };
    if old.is_empty() || old.keys().ne(new.keys()) {
        result.status = RegressionStatus::EvidenceMismatch;
        return result;
    }
    for (id, value) in new {
        let previous = old[id];
        if previous.median > 0.0 {
            let ratio = value.median / previous.median;
            if ratio > 1.20 && value.low > previous.high {
                result.regressed_measurements.push(id.to_string());
                result.worst_ratio = Some(result.worst_ratio.unwrap_or(1.0).max(ratio));
            }
        }
        result.compared_measurements += 1;
    }
    if !result.regressed_measurements.is_empty() {
        result.status = RegressionStatus::Regressed;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(value: f64) -> Snapshot {
        Snapshot {
            version: 1,
            accepted_at: 0.0,
            environment: json!({"host":"same"}),
            source_revision: json!("rev"),
            observations: BTreeMap::from([(
                "[\"search\",\"latency\"]".into(),
                Observation {
                    median: value,
                    low: value,
                    high: value,
                },
            )]),
            promises: BTreeMap::from([("search".into(), PromiseHealth::Pass)]),
            content_fingerprint: String::new(),
        }
    }
    #[test]
    fn regression_does_not_require_missing_absolute_budget() {
        let before = fixture(1.0);
        let mut after = fixture(3.0);
        assert_eq!(
            comparison(&before, &after, "search").status,
            RegressionStatus::Regressed
        );
        after.environment = json!({"host":"other"});
        assert_eq!(
            comparison(&before, &after, "search").status,
            RegressionStatus::EnvironmentMismatch
        );
        after.environment = before.environment.clone();
        after.observations.clear();
        assert_eq!(
            comparison(&before, &after, "search").status,
            RegressionStatus::EvidenceMismatch
        );
    }
    #[test]
    fn overlapping_observed_ranges_do_not_claim_a_regression() {
        let before = fixture(1.0);
        let mut after = fixture(1.3);
        after.observations.values_mut().next().unwrap().low = 0.9;
        assert_eq!(
            comparison(&before, &after, "search").status,
            RegressionStatus::Stable
        );
    }
}
