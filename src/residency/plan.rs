//! One validated workload inventory shared by execution and analysis.
use super::{memory_assessment, validate_policy, LadderArtifact, Trial, RUNGS};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
pub(crate) struct Config {
    version: u32,
    pub probe: Option<String>,
    pub independent_launches: u64,
    memory_profiles: Vec<Option<u64>>,
    operations: BTreeMap<String, Operation>,
}

#[derive(Deserialize)]
pub(crate) struct Operation {
    pub gate_section: String,
    pub gate_id: String,
    workloads: Vec<u64>,
    fixtures: Vec<String>,
    pub workload_unit: String,
    pub completion_endpoint: String,
    pub metric: String,
    pub code_paths: BTreeMap<String, String>,
}

pub(crate) type GroupKey<'a> = (&'a str, u64, &'a str, Option<u64>);

pub(crate) struct Group<'a> {
    pub key: GroupKey<'a>,
    pub operation: &'a Operation,
}

pub(crate) struct Request<'a> {
    pub key: GroupKey<'a>,
    pub launch: u64,
    pub rung: &'static str,
}

impl Request<'_> {
    pub fn matches(&self, trial: &Trial) -> bool {
        self.key == trial.group_key()
            && self.launch == trial.launch_index
            && self.rung == trial.rung
    }
}

impl Trial {
    pub(crate) fn group_key(&self) -> GroupKey<'_> {
        (
            &self.scenario,
            self.workload_value,
            &self.fixture,
            self.memory_profile_bytes,
        )
    }
}

pub(crate) struct Plan<'a> {
    pub config: Config,
    pub manifest: &'a Value,
}

fn unique<T: Ord>(values: &[T]) -> bool {
    !values.is_empty() && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

impl Operation {
    fn validate(&self, manifest: &Value) -> Result<()> {
        ensure!(
            unique(&self.workloads) && self.workloads.iter().all(|n| *n > 0),
            "invalid or duplicate workloads"
        );
        ensure!(unique(&self.fixtures), "invalid or duplicate fixtures");
        ensure!(
            self.fixtures
                .iter()
                .chain([&self.workload_unit, &self.completion_endpoint, &self.metric])
                .all(|s| !s.trim().is_empty()),
            "missing trial contract"
        );
        ensure!(
            RUNGS.iter().all(|r| self
                .code_paths
                .get(*r)
                .is_some_and(|s| !s.trim().is_empty())),
            "missing rung code path"
        );
        ensure!(
            ["capacity_budgets", "resource_budgets"].contains(&self.gate_section.as_str()),
            "invalid reference gate section"
        );
        let budget = &manifest[&self.gate_section][&self.gate_id];
        ensure!(
            budget
                .get("metric")
                .map_or(self.metric == "/elapsed_ns", |metric| metric
                    == &self.metric),
            "reference gate metric mismatch"
        );
        self.limit(manifest)
            .context("diagnostic requires an existing time gate")?;
        Ok(())
    }

    pub fn limit(&self, manifest: &Value) -> Option<f64> {
        let b = &manifest[&self.gate_section][&self.gate_id];
        b["threshold_ms"].as_f64().filter(|n| *n > 0.).or_else(|| {
            b["max"]
                .as_u64()
                .filter(|n| *n > 0 && b["unit"] == "ns")
                .map(|n| n as f64 / 1e6)
        })
    }
}

impl<'a> Plan<'a> {
    pub fn new(manifest: &'a Value, selected: &[String]) -> Result<Self> {
        let mut config = Config::deserialize(&manifest["diagnostics"]["residency"])?;
        ensure!(
            config.version == 1,
            "unsupported residency contract version"
        );
        ensure!(
            (3..=u64::MAX / 5).contains(&config.independent_launches),
            "residency requires at least three independent launches"
        );
        let profiles = &config.memory_profiles;
        ensure!(
            unique(profiles) && profiles.iter().all(|p| p.is_none_or(|n| n > 0)),
            "invalid or duplicate memory profile"
        );
        ensure!(
            [Some(4 << 30), Some(16 << 30), None]
                .iter()
                .all(|p| profiles.contains(p)),
            "required host/4GiB/16GiB profiles absent"
        );
        ensure!(
            !config.operations.is_empty(),
            "empty residency operation inventory"
        );
        for operation in config.operations.values() {
            operation.validate(manifest)?;
        }
        ensure!(
            selected.iter().all(|id| config.operations.contains_key(id)),
            "unknown residency scenario"
        );
        config
            .operations
            .retain(|id, _| selected.is_empty() || selected.contains(id));
        validate_policy(&manifest["durability_policy"])?;
        Ok(Self { config, manifest })
    }

    pub fn groups(&self) -> impl Iterator<Item = Group<'_>> {
        self.config
            .operations
            .iter()
            .flat_map(move |(id, operation)| {
                operation.workloads.iter().flat_map(move |size| {
                    operation.fixtures.iter().flat_map(move |fixture| {
                        self.config
                            .memory_profiles
                            .iter()
                            .map(move |profile| Group {
                                key: (id, *size, fixture, *profile),
                                operation,
                            })
                    })
                })
            })
    }

    pub fn requests(&self) -> impl Iterator<Item = Request<'_>> {
        self.groups().flat_map(move |group| {
            (0..self.config.independent_launches).flat_map(move |launch| {
                // Rotate order while preserving five paired arms in every launch.
                (0..5).map(move |offset| Request {
                    key: group.key,
                    launch,
                    rung: RUNGS[(offset + (launch % 5) as usize) % 5],
                })
            })
        })
    }

    pub fn validate_trial(&self, trial: &Trial) -> Result<()> {
        let op = self
            .config
            .operations
            .get(&trial.scenario)
            .context("unsolicited diagnostic scenario")?;
        ensure!(
            self.config
                .memory_profiles
                .contains(&trial.memory_profile_bytes),
            "undeclared memory profile"
        );
        ensure!(
            trial.launch_index < self.config.independent_launches,
            "unexpected launch index"
        );
        ensure!(
            op.workloads.contains(&trial.workload_value),
            "undeclared workload"
        );
        ensure!(op.fixtures.contains(&trial.fixture), "undeclared fixture");
        super::validate_trial(trial, op)
    }
}

impl Plan<'_> {
    pub fn analyze(&self, trials: Vec<Trial>) -> Result<LadderArtifact> {
        let mut seen = BTreeSet::new();
        let mut by_group = BTreeMap::<_, Vec<_>>::new();
        for t in &trials {
            self.validate_trial(t)?;
            ensure!(
                trials
                    .first()
                    .is_some_and(|first| first.revision == t.revision),
                "mixed revisions in one ladder"
            );
            ensure!(
                seen.insert((t.group_key(), t.launch_index, t.rung.as_str())),
                "duplicate trial"
            );
            by_group.entry(t.group_key()).or_default().push(t);
        }
        let mut gaps = Vec::new();
        let mut groups = Vec::new();
        for group in self.groups() {
            let rows = by_group
                .get(&group.key)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let report = group_report(&group, self, rows)?;
            if report["complete"] != true {
                let (id, size, fixture, profile) = group.key;
                gaps.push(format!(
                    "{id}/{size}/{}/{profile}: incomplete or unavailable rungs",
                    json!(fixture),
                    profile = json!(profile)
                ));
            }
            groups.push(report);
        }
        Ok(LadderArtifact {version:1,source_context:Value::Null,diagnostic_only:true,promise_eligible:false,complete:gaps.is_empty(),gaps,groups,trials,
            interpretation:"Paired diagnostic contrasts are not exclusive causal phase costs. Budget simulation does not constrain physical RAM. Instrument counts are companion signals; instrumented timings cannot establish latency gates.".into()})
    }
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some((values[values.len() / 2] + values[(values.len() - 1) / 2]) / 2.)
}

fn paired_results(rows: &[&Trial], launches: u64) -> Result<()> {
    for launch in 0..launches {
        let mut pair = rows
            .iter()
            .filter(|t| t.launch_index == launch && t.status == "ok");
        if let Some(first) = pair.next() {
            ensure!(
                pair.all(|t| t.fixture_digest == first.fixture_digest
                    && t.result_digest == first.result_digest
                    && t.result_count == first.result_count
                    && t.expected_result_count == first.expected_result_count
                    && t.host_memory_bytes == first.host_memory_bytes),
                "paired rungs disagree on fixture/results/host"
            );
        }
    }
    Ok(())
}

fn classification(
    complete: bool,
    stable: bool,
    medians: &[Option<f64>; 5],
    limit: f64,
) -> &'static str {
    if !complete {
        return "incomplete";
    }
    if !stable {
        return "uncertain_noise";
    }
    let causes = [
        "computation",
        "piece_tree_or_window_assembly",
        "resident_store_access",
        "retrieval_or_residency",
        "retrieval_or_residency",
    ];
    medians
        .iter()
        .zip(causes)
        .find_map(|(m, cause)| m.filter(|v| *v > limit).map(|_| cause))
        .unwrap_or("all_diagnostic_rungs_within_budget")
}

fn contrasts(rows: &[&Trial], launches: u64) -> Vec<Value> {
    let timing = |launch, rung| {
        rows.iter()
            .find(|t| t.launch_index == launch && t.rung == rung && t.status == "ok")?
            .metric_ns
    };
    RUNGS
        .windows(2)
        .map(|pair| {
            let differences = (0..launches)
                .filter_map(|launch| {
                    Some((timing(launch, pair[1])? as f64 - timing(launch, pair[0])? as f64) / 1e6)
                })
                .collect();
            json!({"from":pair[0],"to":pair[1],"paired_median_difference_ms":median(differences)})
        })
        .collect()
}

fn group_report(group: &Group<'_>, plan: &Plan<'_>, rows: &[&Trial]) -> Result<Value> {
    let (id, size, fixture, profile) = group.key;
    let op = group.operation;
    let launches = plan.config.independent_launches;
    let budget = &plan.manifest[&op.gate_section][&op.gate_id];
    let limit = op
        .limit(plan.manifest)
        .context("missing reference time budget")?;
    paired_results(rows, launches)?;
    let complete = rows.len() as u64 == launches * 5 && rows.iter().all(|t| t.status == "ok");
    let stable = rows.iter().all(|t| {
        crate::environment::quality(
            &t.measurement_quality["before"],
            &t.measurement_quality["after"],
        )["status"]
            == "stable"
    });
    let medians = RUNGS.map(|r| {
        median(
            rows.iter()
                .filter(|t| t.rung == r && t.status == "ok")
                .filter_map(|t| t.metric_ns.map(|n| n as f64 / 1e6))
                .collect(),
        )
    });
    Ok(
        json!({"scenario":id,"workload_value":size,"fixture":fixture,"memory_profile_bytes":profile,
        "completion_endpoint":op.completion_endpoint,"reference_gate_endpoint":budget["outcome"]["endpoint"],"reference_gate":op.gate_id,
        "budget_ms":limit,"complete":complete,"stable":stable,"classification":classification(complete,stable,&medians,limit),
        "rungs":RUNGS.iter().zip(medians).map(|(r,m)|json!({"rung":r,"median_ms":m})).collect::<Vec<_>>(),"contrasts":contrasts(rows,launches),
        "memory":rows.iter().map(|t|memory_assessment(&t.memory,&plan.manifest["durability_policy"])).collect::<Result<Vec<_>>>()?}),
    )
}
