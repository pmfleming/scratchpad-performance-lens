//! Accepted, content-bound snapshots spanning every promise and evidence axis.
use crate::{
    artifacts::performance_review::{
        BaselineComparison, CoverageStatus, PerformanceReviewArtifact, PerformanceReviewScenario,
        PromiseHealth, RegressionStatus,
    },
    config::LensConfig,
    provenance,
};
use anyhow::{ensure, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

#[derive(Debug, Serialize, Deserialize)]
struct Observation {
    high: f64,
    low: f64,
    median: f64,
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

fn fingerprint(snapshot: &Snapshot) -> Result<String> {
    // Field order matches the original JSON object's canonical, sorted keys.
    #[derive(Serialize)]
    struct Content<'a> {
        environment: &'a Value,
        observations: &'a BTreeMap<String, Observation>,
        promises: &'a BTreeMap<String, PromiseHealth>,
        revision: &'a Value,
    }
    Ok(provenance::fingerprint(&serde_json::to_vec(&Content {
        environment: &snapshot.environment,
        observations: &snapshot.observations,
        promises: &snapshot.promises,
        revision: &snapshot.source_revision,
    })?))
}

fn read(path: &Path) -> Result<Snapshot> {
    let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        snapshot.version == 1 && snapshot.content_fingerprint == fingerprint(&snapshot)?,
        "invalid baseline fingerprint/version"
    );
    Ok(snapshot)
}

pub fn accept(args: BaselineArgs) -> Result<()> {
    let config = LensConfig::load(args.config.as_deref())?;
    let report = load_current(&config)?;
    ensure!(
        report.meta.noisy_sources.is_empty(),
        "noisy runs cannot be accepted or compared as baselines"
    );
    ensure!(
        report.scenarios.len() == report.promise_count && report.promise_count > 0,
        "incomplete promise inventory"
    );
    ensure!(report.scenarios.iter().all(|s| s.coverage_status == CoverageStatus::Covered && !s.synthetic_evidence),
        "acceptance requires complete real evidence for every promise; budget failures may be accepted as baseline observations");
    let context = provenance::context(&config.project_root);
    let mut snapshot = snapshot(&context, &report.scenarios)?;
    ensure!(
        !snapshot.observations.is_empty(),
        "no valid baseline observations"
    );
    snapshot.content_fingerprint = fingerprint(&snapshot)?;
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
    ensure!(
        report.meta.noisy_sources.is_empty(),
        "noisy runs cannot be accepted or compared as baselines"
    );
    let baseline = read(&args.baseline.unwrap_or_else(|| default_path(&config)))?;
    let current = snapshot(
        &provenance::context(&config.project_root),
        &report.scenarios,
    )?;
    let comparisons: BTreeMap<_, _> = report
        .scenarios
        .iter()
        .map(|s| Ok((s.id.clone(), comparison(&baseline, &current, &s.id)?)))
        .collect::<Result<_>>()?;
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
    let current = snapshot(context, scenarios)?;
    for scenario in scenarios {
        scenario.baseline_comparison = comparison(&baseline, &current, &scenario.id)?;
        if scenario.baseline_comparison.status == RegressionStatus::Regressed
            && scenario.promise_health == PromiseHealth::Pass
        {
            scenario.promise_health = PromiseHealth::PassingButRegressed;
        }
    }
    Ok(())
}

struct Measurement<'a> {
    axis: &'static str,
    id: &'a str,
    sample: &'a Value,
    workload: &'a Value,
}

impl Measurement<'_> {
    fn eligible(&self) -> bool {
        let sample = self.sample;
        let measured = match self.axis {
            "latency" => sample["measurement_status"] == "valid",
            "capacity" => sample["status"] == "ok" && sample["complete"] == true,
            _ => sample["status"] == "ok",
        };
        measured && crate::measurement_contract::eligible(self.id, &sample["measurement_contract"])
    }

    fn metrics(&self) -> (&str, Option<&str>) {
        match self.axis {
            "latency" if self.sample["evidence_kind"] == "frame" => ("p95_ms", Some("p99_ms")),
            "latency" => ("budget_probe_ns", None),
            "capacity" => ("elapsed_ms", None),
            _ => (
                self.sample["resource_budget"]["metric"]
                    .as_str()
                    .unwrap_or("/elapsed_ns"),
                None,
            ),
        }
    }

    fn record(&self, promise: &str, raw: &mut BTreeMap<String, Vec<Observation>>) -> Result<()> {
        let (primary, tail) = self.metrics();
        for metric in std::iter::once(primary).chain(tail) {
            if let Some(value) = self.observation(metric) {
                let key = serde_json::to_string(&(
                    promise,
                    self.axis,
                    self.id,
                    self.workload,
                    &self.sample["measurement_contract"],
                    &self.sample["measurement_scope"],
                    metric,
                ))?;
                raw.entry(key).or_default().push(value);
            }
        }
        Ok(())
    }

    fn observation(&self, metric: &str) -> Option<Observation> {
        let sample = self.sample;
        let value = sample
            .get(metric)
            .or_else(|| sample.pointer(metric))?
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)?;
        let (low, high) = match self.axis {
            "latency" => (
                &sample["mean_confidence_interval_ns"]["lower_bound"],
                &sample["mean_confidence_interval_ns"]["upper_bound"],
            ),
            "capacity" => (&sample["elapsed_min_ms"], &sample["elapsed_max_ms"]),
            _ => (&Value::Null, &Value::Null),
        };
        Some(Observation {
            median: value,
            low: low.as_f64().unwrap_or(value),
            high: high.as_f64().unwrap_or(value),
        })
    }
}

fn nested_measurements<'a>(
    rows: &'a [Value],
    axis: &'static str,
) -> impl Iterator<Item = Measurement<'a>> {
    rows.iter().flat_map(move |row| {
        row["samples"]
            .as_array()
            .into_iter()
            .flatten()
            .map(move |sample| Measurement {
                axis,
                id: row["scenario"].as_str().unwrap_or(""),
                sample,
                workload: &sample["workload_value"],
            })
    })
}

fn snapshot(context: &Value, scenarios: &[PerformanceReviewScenario]) -> Result<Snapshot> {
    let mut raw = BTreeMap::new();
    for scenario in scenarios {
        let latency = scenario.evidence.latency.iter().map(|sample| Measurement {
            axis: "latency",
            id: sample["benchmark_key"].as_str().unwrap_or(""),
            workload: &sample["parameter_value"],
            sample,
        });
        let measurements = latency
            .chain(nested_measurements(&scenario.evidence.capacity, "capacity"))
            .chain(nested_measurements(
                &scenario.evidence.resources,
                "resource",
            ));
        for measurement in measurements.filter(Measurement::eligible) {
            measurement.record(&scenario.id, &mut raw)?;
        }
    }
    let observations = raw
        .into_iter()
        .map(|(key, mut values)| {
            values.sort_by(|a, b| a.median.total_cmp(&b.median));
            let median = values[values.len() / 2].median;
            (
                key,
                Observation {
                    median,
                    low: values.iter().map(|v| v.low).fold(f64::INFINITY, f64::min),
                    high: values.iter().map(|v| v.high).fold(0.0, f64::max),
                },
            )
        })
        .collect();
    Ok(Snapshot {
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
    })
}

fn comparison(before: &Snapshot, after: &Snapshot, promise: &str) -> Result<BaselineComparison> {
    let mut result = BaselineComparison::default();
    if before.environment != after.environment {
        result.status = RegressionStatus::EnvironmentMismatch;
        return Ok(result);
    }
    let prefix = format!("[{}", serde_json::to_string(promise)?);
    let old = before
        .observations
        .iter()
        .filter(|(id, _)| id.starts_with(&prefix));
    let new = after
        .observations
        .iter()
        .filter(|(id, _)| id.starts_with(&prefix));
    if old.clone().next().is_none()
        || old
            .clone()
            .map(|(id, _)| id)
            .ne(new.clone().map(|(id, _)| id))
    {
        result.status = RegressionStatus::EvidenceMismatch;
        return Ok(result);
    }
    for ((id, previous), (_, value)) in old.zip(new) {
        if previous.median > 0.0
            && value.median / previous.median > 1.20
            && value.low > previous.high
        {
            result.regressed_measurements.push(id.to_owned());
            result.worst_ratio = Some(
                result
                    .worst_ratio
                    .unwrap_or(1.0)
                    .max(value.median / previous.median),
            );
        }
        result.compared_measurements += 1;
    }
    result.status = if result.regressed_measurements.is_empty() {
        RegressionStatus::Stable
    } else {
        RegressionStatus::Regressed
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        accept, annotate, compare, comparison, fingerprint, read, snapshot, BaselineArgs, Snapshot,
    };
    use crate::{
        artifacts::performance_review::{
            PerformanceReviewArtifact, PromiseHealth, RegressionStatus,
        },
        config::LensConfig,
        provenance,
    };
    use anyhow::Result;
    use serde_json::{json, Value};
    use std::{fs, path::Path};

    fn report(value: f64) -> Result<PerformanceReviewArtifact> {
        let axis = json!({"required":true,"covered":true,"count":1});
        let class = json!({"label":"fixture","purpose":"fixture","count":1});
        let contract = json!({"text_backing":"owned","production_path":true,"code_path":"fixture"});
        let capacity = json!({"scenario":"tab_count_ceiling","samples":[{
            "status":"ok","complete":true,"workload_value":10000,
            "elapsed_ms":value,"elapsed_min_ms":value,"elapsed_max_ms":value}]});
        let evidence = json!({"profiles":[],"capacity":[capacity],"resources":[{"scenario":"fixture","samples":[{
            "status":"ok","workload_value":1,"measurement_contract":contract,"phase_elapsed_ns":{"save_ns":value},
            "resource_budget":{"metric":"/phase_elapsed_ns/save_ns"}}]}],
            "latency":[{"benchmark_key":"fixture","parameter_value":1,"measurement_status":"valid","budget_probe_ns":value,
                "measurement_contract":contract,"measurement_scope":"fixture"},
                {"benchmark_key":"fixture_frame","measurement_status":"valid","evidence_kind":"frame","p95_ms":value,"p99_ms":value},
                {"measurement_status":"invalid"}]});
        let scenario = json!({"id":"search","title":"Search","promise":"Search","promise_health":"pass",
            "synthetic_evidence":false,"coverage_status":"covered","coverage_score":1.0,
            "axes":{"speed":axis,"capacity":axis,"resource":axis,"profiles":axis},
            "promised_scale":[],"scale_checks":[],"gaps":[],"opportunities":[],"implementations":[],"implementation_count":0,
            "measurement_split":{"ceiling_health":class,"targeted_path":class,"diagnostic_profile":class},
            "budget_misses":0,"ceilings_reached":0,"scale_failures":0,"evidence":evidence});
        let summary = json!({"scenario_count":1,"covered_scenarios":1,"thin_scenarios":0,"missing_scenarios":0,
            "coverage_gaps":0,"missing_scale_targets":0,"budget_misses":0,"ceilings_reached":0,"implementation_count":0,
            "ceiling_health_probes":1,"targeted_path_probes":2,"latency_rows":2,"capacity_rows":1,"resource_rows":1,"flamegraph_rows":0,
            "promise_health":{"pass":1,"at_risk":0,"fail":0,"unmeasured":0}});
        serde_json::from_value(json!({
            "meta":{"generated_from":"test","source_artifacts":[],"scenario_model":"fixture","probe_classes":{}},
            "summary":summary,"target_promises":["search"],"promise_count":1,"promise_registry":[],"opportunities":[],"coordinated_triage":[],
            "presentation":{"notes":[],"primary_view":"fixture","secondary_view":"fixture"},"scenarios":[scenario]
        })).map_err(Into::into)
    }

    fn publish(config: &LensConfig, report: &PerformanceReviewArtifact) -> Result<()> {
        fs::write(
            config.output_dir.join("performance_review.json"),
            serde_json::to_vec(report)?,
        )?;
        provenance::record(
            config,
            "performance_review.json",
            0.0,
            &provenance::context(&config.project_root),
            true,
        )
    }
    fn args(path: &Path) -> BaselineArgs {
        BaselineArgs {
            config: Some(path.to_owned()),
            baseline: None,
        }
    }

    #[test]
    fn accepted_baseline_lifecycle_preserves_identity_budgets_and_archive_integrity() -> Result<()>
    {
        let root = tempfile::tempdir()?;
        let path = root.path().join("splens.toml");
        fs::write(
            &path,
            format!(
                "project_root = {:?}\noutput_dir = 'target/analysis'\n",
                root.path()
            ),
        )?;
        let config = LensConfig::load(Some(&path))?;
        fs::create_dir_all(&config.output_dir)?;
        assert!(accept(args(&path)).is_err());
        let mut candidate = report(1.0)?;
        let context = provenance::context(root.path());
        annotate(&config, &context, &mut candidate.scenarios)?;
        publish(&config, &candidate)?;
        accept(args(&path))?;
        compare(args(&path))?;
        let baseline_path = config.output_dir.join("baselines/accepted.json");
        let before = read(&baseline_path)?;
        assert_eq!(before.observations.len(), 5);
        candidate = report(3.0)?;
        publish(&config, &candidate)?;
        assert!(compare(args(&path)).is_err());
        annotate(&config, &context, &mut candidate.scenarios)?;
        assert_eq!(
            candidate.scenarios[0].promise_health,
            PromiseHealth::PassingButRegressed
        );
        candidate.scenarios[0].promise_health = PromiseHealth::Fail;
        annotate(&config, &context, &mut candidate.scenarios)?;
        assert_eq!(candidate.scenarios[0].promise_health, PromiseHealth::Fail);
        let mut after = snapshot(&context, &candidate.scenarios)?;
        for row in after.observations.values_mut() {
            row.low = 0.5;
        }
        assert_eq!(
            comparison(&before, &after, "search")?.status,
            RegressionStatus::Stable
        );
        after.environment = Value::Null;
        assert_eq!(
            comparison(&before, &after, "search")?.status,
            RegressionStatus::EnvironmentMismatch
        );
        after.environment = before.environment;
        after.observations.clear();
        let prior = read(&baseline_path)?;
        assert_eq!(
            comparison(&prior, &after, "search")?.status,
            RegressionStatus::EvidenceMismatch
        );
        for invalid in ["noise", "coverage", "inventory", "synthetic"] {
            let mut bad = report(1.0)?;
            match invalid {
                "noise" => bad.meta.noisy_sources.push("fixture".into()),
                "coverage" => {
                    bad.scenarios[0].coverage_status =
                        crate::artifacts::performance_review::CoverageStatus::Thin
                }
                "inventory" => bad.promise_count = 0,
                _ => bad.scenarios[0].synthetic_evidence = true,
            }
            publish(&config, &bad)?;
            assert!(accept(args(&path)).is_err(), "{invalid}");
        }
        publish(&config, &report(3.0)?)?;
        accept(args(&path))?;
        assert_eq!(
            fs::read_dir(config.output_dir.join("baselines"))?.count(),
            2
        );
        let legacy: Snapshot = serde_json::from_value(
            json!({"version":1,"accepted_at":0.0,"environment":{"host":"same"},
            "source_revision":"rev","observations":{"[\"search\",\"latency\"]":{"median":1.0,"low":1.0,"high":1.0}},
            "promises":{"search":"pass"},"content_fingerprint":""}),
        )?;
        assert_eq!(fingerprint(&legacy)?, "9d3a7272f36621b5");
        let mut corrupt: Value = serde_json::from_slice(&fs::read(&baseline_path)?)?;
        corrupt["version"] = json!(2);
        fs::write(&baseline_path, corrupt.to_string())?;
        assert!(read(&baseline_path).is_err());
        fs::write(config.output_dir.join("performance_review.json"), "{}")?;
        assert!(compare(args(&path)).is_err());
        Ok(())
    }
}
