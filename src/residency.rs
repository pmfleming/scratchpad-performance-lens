//! Opt-in diagnostic contracts. No diagnostic row can establish a promise pass.
use anyhow::{ensure, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const RUNGS: [&str; 5] = [
    "owned_contiguous",
    "owned_piece_tree",
    "store_resident",
    "store_warm",
    "store_cold",
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Trial {
    pub scenario: String,
    pub workload_value: u64,
    pub workload_unit: String,
    pub fixture: String,
    pub fixture_digest: String,
    pub result_digest: String,
    pub result_count: u64,
    pub expected_result_count: u64,
    pub code_path: String,
    pub completion_endpoint: String,
    pub revision: String,
    pub rung: String,
    /// Budget simulation only; null denotes the host profile.
    pub memory_profile_bytes: Option<u64>,
    pub host_memory_bytes: u64,
    pub launch_index: u64,
    pub metric_ns: Option<u64>,
    pub metric: String,
    pub timing_source: String,
    pub status: String,
    pub unavailable_reason: Option<String>,
    pub cancellation_checked: bool,
    pub observations: Value,
    pub memory: Value,
    pub measurement_quality: Value,
    /// Optional named Callgrind phase counts. Never compare instrumented wall time.
    pub instructions: BTreeMap<String, u64>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LadderArtifact {
    pub version: u32,
    #[serde(default)]
    pub source_context: Value,
    pub diagnostic_only: bool,
    pub promise_eligible: bool,
    pub complete: bool,
    pub gaps: Vec<String>,
    pub groups: Vec<Value>,
    pub trials: Vec<Trial>,
    pub interpretation: String,
}

fn positive(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n > 0)
}

pub fn validate_policy(policy: &Value) -> Result<()> {
    ensure!(
        policy["version"] == 1 && positive(&policy["pending_queue_limit_bytes"]).is_some(),
        "invalid durability policy"
    );
    ensure!(
        policy["manifest_sync"] == "FULL" && policy["body_sync"] == "NORMAL",
        "unexpected commit policy: review policy changes explicitly"
    );
    ensure!(
        policy["maximum_exposure_ms"].is_null()
            || positive(&policy["maximum_exposure_ms"]).is_some(),
        "invalid exposure limit"
    );
    Ok(())
}

pub fn memory_assessment(memory: &Value, policy: &Value) -> Result<Value> {
    validate_policy(policy)?;
    ensure!(
        memory.is_null() || memory.is_object(),
        "invalid memory observations"
    );
    for key in [
        "publication_timestamp_ns",
        "full_commit_timestamp_ns",
        "configured_pending_limit_bytes",
    ] {
        ensure!(
            memory[key].is_null() || memory[key].as_u64().is_some(),
            "invalid observation: {key}"
        );
    }
    for (current, peak) in [
        ("resident_durable_bytes", "resident_durable_peak_bytes"),
        (
            "accepted_not_durable_bytes",
            "accepted_not_durable_peak_bytes",
        ),
        ("scratch_bytes", "scratch_peak_bytes"),
    ] {
        for key in [current, peak] {
            ensure!(
                memory[key].is_null() || memory[key].as_u64().is_some(),
                "invalid byte observation: {key}"
            );
        }
        if let (Some(c), Some(p)) = (memory[current].as_u64(), memory[peak].as_u64()) {
            ensure!(c <= p, "{current} exceeds peak");
        }
    }
    let publication = memory["publication_timestamp_ns"].as_u64();
    let full_commit = memory["full_commit_timestamp_ns"].as_u64();
    let exposure = match (publication, full_commit) {
        (Some(p), Some(c)) => Some(
            c.checked_sub(p)
                .context("FULL commit precedes publication")? as f64
                / 1e6,
        ),
        _ => None,
    };
    let configured = memory["configured_pending_limit_bytes"].as_u64();
    let policy_change =
        configured.map(|n| n != policy["pending_queue_limit_bytes"].as_u64().unwrap());
    Ok(json!({"metrics":memory,"exposure_window_ms":exposure,
        "policy_change":policy_change,
        "classification":match policy_change {Some(true)=>"durability_policy_change",Some(false)=>"declared_pending_policy",None=>"unknown_policy"},
        "exposure_limit_exceeded": exposure.zip(policy["maximum_exposure_ms"].as_f64()).map(|(a,b)| a>b),
        "missing_metrics":(["resident_durable_bytes","resident_durable_peak_bytes","accepted_not_durable_bytes","accepted_not_durable_peak_bytes","scratch_peak_bytes"].into_iter().filter(|k|memory[*k].is_null()).collect::<Vec<_>>()),
        "interpretation":"NORMAL body commit is separate from FULL durability; pending queue is not total accepted uncommitted text; unknown metrics are not zero"}))
}

fn validate_trial(t: &Trial, spec: &Value) -> Result<()> {
    ensure!(RUNGS.contains(&t.rung.as_str()), "unknown rung");
    ensure!(
        t.workload_value > 0 && !t.revision.is_empty() && !t.code_path.is_empty(),
        "missing trial identity"
    );
    ensure!(
        spec["workloads"]
            .as_array()
            .is_some_and(|v| v.contains(&json!(t.workload_value))),
        "undeclared workload"
    );
    ensure!(
        spec["fixtures"]
            .as_array()
            .is_some_and(|v| v.contains(&json!(t.fixture))),
        "undeclared fixture"
    );
    ensure!(
        t.workload_unit == spec["workload_unit"].as_str().unwrap_or("")
            && t.completion_endpoint == spec["completion_endpoint"].as_str().unwrap_or(""),
        "workload unit or completion endpoint mismatch"
    );
    ensure!(
        t.timing_source == "native_wall",
        "instrumented timings cannot be used in residency latency comparisons"
    );
    ensure!(
        spec["code_paths"][&t.rung].as_str() == Some(t.code_path.as_str()),
        "unexpected diagnostic code path"
    );
    ensure!(
        t.host_memory_bytes > 0 && t.memory_profile_bytes.is_none_or(|n| n > 0),
        "invalid memory profile"
    );
    ensure!(
        t.fixture_digest.len() == 64 && t.fixture_digest.bytes().all(|c| c.is_ascii_hexdigit()),
        "fixture requires SHA-256 digest"
    );
    if t.status == "unavailable" {
        ensure!(
            t.metric_ns.is_none() && t.unavailable_reason.as_ref().is_some_and(|s| !s.is_empty()),
            "unavailable trials need a reason, not a timing"
        );
        return Ok(());
    }
    ensure!(
        t.status == "ok" && t.metric_ns.is_some_and(|n| n > 0),
        "failed or missing trial timing"
    );
    ensure!(
        t.result_digest.len() == 64
            && t.result_digest.bytes().all(|c| c.is_ascii_hexdigit())
            && t.result_count == t.expected_result_count
            && t.cancellation_checked,
        "missing exact-result or cancellation evidence"
    );
    ensure!(
        Some(t.metric.as_str()) == spec["metric"].as_str(),
        "diagnostic metric mismatch"
    );
    let o = &t.observations;
    if t.metric.contains("p99") {
        let samples = o["metric_samples_ns"]
            .as_array()
            .context("p99 requires raw samples")?;
        let mut values = samples
            .iter()
            .map(|v| v.as_u64().filter(|n| *n > 0))
            .collect::<Option<Vec<_>>>()
            .context("invalid p99 samples")?;
        ensure!(values.len() >= 128, "too few p99 samples");
        values.sort_unstable();
        ensure!(
            t.metric_ns == Some(values[(values.len() * 99).div_ceil(100) - 1]),
            "incorrect p99 statistic"
        );
    }
    match t.rung.as_str() {
        "owned_contiguous" | "owned_piece_tree" => ensure!(
            o["store_read_count"] == 0,
            "owned reference performed store reads"
        ),
        "store_resident" => ensure!(
            o["store_read_count"] == 0 && positive(&o["retained_read_count"]).is_some(),
            "resident rung performed store reads or had no cache hits"
        ),
        "store_warm" => ensure!(
            o["app_cache_empty_at_start"] == true
                && o["os_cache_primed"] == true
                && positive(&o["store_read_count"]).is_some(),
            "warm rung lacks setup/read proof"
        ),
        "store_cold" => ensure!(
            o["app_cache_empty_at_start"] == true
                && o["eviction_succeeded"] == true
                && positive(&o["physical_read_bytes"]).is_some(),
            "cold rung lacks physical read proof"
        ),
        _ => unreachable!(),
    }
    Ok(())
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some((values[values.len() / 2] + values[(values.len() - 1) / 2]) / 2.)
}

pub fn analyze(
    manifest: &Value,
    trials: Vec<Trial>,
    selected: &[String],
) -> Result<LadderArtifact> {
    let config = &manifest["diagnostics"]["residency"];
    ensure!(
        config["version"] == 1,
        "unsupported residency contract version"
    );
    let specs = config["operations"]
        .as_object()
        .context("missing residency operation declarations")?;
    let launches = positive(&config["independent_launches"])
        .filter(|n| *n >= 3)
        .context("residency requires at least three independent launches")?;
    let profiles = config["memory_profiles"]
        .as_array()
        .context("missing memory profiles")?;
    ensure!(
        profiles.contains(&Value::Null)
            && profiles.contains(&json!(4u64 * 1024 * 1024 * 1024))
            && profiles.contains(&json!(16u64 * 1024 * 1024 * 1024)),
        "required host/4GiB/16GiB profiles absent"
    );
    ensure!(
        profiles
            .iter()
            .all(|p| p.is_null() || positive(p).is_some())
            && profiles
                .iter()
                .map(Value::to_string)
                .collect::<BTreeSet<_>>()
                .len()
                == profiles.len(),
        "invalid or duplicate memory profile"
    );
    ensure!(!specs.is_empty(), "empty residency operation inventory");
    for (id, spec) in specs {
        let workloads = spec["workloads"].as_array().context("missing workloads")?;
        let fixtures = spec["fixtures"].as_array().context("missing fixtures")?;
        ensure!(
            !workloads.is_empty()
                && workloads.iter().all(|v| positive(v).is_some())
                && workloads
                    .iter()
                    .map(Value::to_string)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == workloads.len(),
            "{id}: invalid or duplicate workloads"
        );
        ensure!(
            !fixtures.is_empty()
                && fixtures
                    .iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
                && fixtures
                    .iter()
                    .map(Value::to_string)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == fixtures.len(),
            "{id}: invalid or duplicate fixtures"
        );
        ensure!(
            ["workload_unit", "completion_endpoint", "metric"]
                .iter()
                .all(|key| spec[*key].as_str().is_some_and(|s| !s.trim().is_empty()))
                && RUNGS.iter().all(|r| spec["code_paths"][*r]
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty())),
            "{id}: missing trial contract"
        );
        let section = spec["gate_section"]
            .as_str()
            .context("missing gate section")?;
        let gate = spec["gate_id"].as_str().context("missing gate id")?;
        ensure!(
            ["capacity_budgets", "resource_budgets"].contains(&section),
            "invalid reference gate section"
        );
        let budget = &manifest[section][gate];
        ensure!(
            spec["metric"]
                == budget
                    .get("metric")
                    .cloned()
                    .unwrap_or(json!("/elapsed_ns")),
            "{id}: reference gate metric mismatch"
        );
        ensure!(
            budget["threshold_ms"].as_f64().is_some_and(|n| n > 0.)
                || (budget["unit"] == "ns" && positive(&budget["max"]).is_some()),
            "{id}: diagnostic requires an existing time gate"
        );
    }
    let selection: BTreeSet<_> = if selected.is_empty() {
        specs.keys().cloned().collect()
    } else {
        selected.iter().cloned().collect()
    };
    ensure!(
        selection.iter().all(|id| specs.contains_key(id)),
        "unknown residency scenario"
    );
    validate_policy(&manifest["durability_policy"])?;
    let mut seen = BTreeSet::new();
    let mut revision = None;
    for t in &trials {
        ensure!(
            selection.contains(&t.scenario),
            "unsolicited diagnostic scenario"
        );
        ensure!(
            profiles.contains(&json!(t.memory_profile_bytes)),
            "undeclared memory profile"
        );
        ensure!(t.launch_index < launches, "unexpected launch index");
        validate_trial(t, &specs[&t.scenario])?;
        memory_assessment(&t.memory, &manifest["durability_policy"])?;
        let r = revision.get_or_insert(&t.revision);
        ensure!(*r == &t.revision, "mixed revisions in one ladder");
        ensure!(
            seen.insert((
                t.scenario.clone(),
                t.workload_value,
                t.fixture.clone(),
                t.memory_profile_bytes,
                t.launch_index,
                t.rung.clone()
            )),
            "duplicate trial"
        );
    }
    let mut gaps = Vec::new();
    let mut groups = Vec::new();
    for id in selection {
        let spec = &specs[&id];
        let budget = manifest[spec["gate_section"]
            .as_str()
            .context("missing gate section")?]
            [spec["gate_id"].as_str().context("missing gate id")?]
        .clone();
        let limit = budget["threshold_ms"]
            .as_f64()
            .or_else(|| {
                budget["max"]
                    .as_f64()
                    .filter(|_| budget["unit"] == "ns")
                    .map(|n| n / 1e6)
            })
            .context("diagnostic requires an existing time gate")?;
        for size in spec["workloads"].as_array().context("missing workloads")? {
            for fixture in spec["fixtures"].as_array().context("missing fixtures")? {
                for profile in profiles {
                    let rows: Vec<_> = trials
                        .iter()
                        .filter(|t| {
                            t.scenario == id
                                && json!(t.workload_value) == *size
                                && json!(t.fixture) == *fixture
                                && json!(t.memory_profile_bytes) == *profile
                        })
                        .collect();
                    let complete =
                        rows.len() as u64 == launches * 5 && rows.iter().all(|t| t.status == "ok");
                    if !complete {
                        gaps.push(format!(
                            "{id}/{size}/{fixture}/{profile}: incomplete or unavailable rungs"
                        ));
                    }
                    for launch in 0..launches {
                        let pair: Vec<_> = rows
                            .iter()
                            .filter(|t| t.launch_index == launch && t.status == "ok")
                            .collect();
                        if let Some(first) = pair.first() {
                            ensure!(
                                pair.iter().all(|t| t.fixture_digest == first.fixture_digest
                                    && t.result_digest == first.result_digest
                                    && t.result_count == first.result_count
                                    && t.expected_result_count == first.expected_result_count
                                    && t.host_memory_bytes == first.host_memory_bytes),
                                "paired rungs disagree on fixture/results/host"
                            );
                        }
                    }
                    let stable = rows.iter().all(|t| {
                        crate::environment::quality(
                            &t.measurement_quality["before"],
                            &t.measurement_quality["after"],
                        )["status"]
                            == "stable"
                    });
                    let medians: Vec<_> = RUNGS
                        .iter()
                        .map(|r| {
                            median(
                                rows.iter()
                                    .filter(|t| t.rung == *r && t.status == "ok")
                                    .filter_map(|t| t.metric_ns.map(|n| n as f64 / 1e6))
                                    .collect(),
                            )
                        })
                        .collect();
                    let classification = if !complete {
                        "incomplete"
                    } else if !stable {
                        "uncertain_noise"
                    } else if medians[0].unwrap() > limit {
                        "computation"
                    } else if medians[1].unwrap() > limit {
                        "piece_tree_or_window_assembly"
                    } else if medians[2].unwrap() > limit {
                        "resident_store_access"
                    } else if medians[3].unwrap() > limit || medians[4].unwrap() > limit {
                        "retrieval_or_residency"
                    } else {
                        "all_diagnostic_rungs_within_budget"
                    };
                    let contrasts:Vec<_>=[(0,1),(1,2),(2,3),(3,4)].iter().map(|(a,b)| {
                        let differences=(0..launches).filter_map(|i|{
                            let left=rows.iter().find(|t|t.launch_index==i && t.rung==RUNGS[*a] && t.status=="ok")?.metric_ns?;
                            let right=rows.iter().find(|t|t.launch_index==i && t.rung==RUNGS[*b] && t.status=="ok")?.metric_ns?;
                            Some((right as f64-left as f64)/1e6)
                        }).collect();
                        json!({"from":RUNGS[*a],"to":RUNGS[*b],"paired_median_difference_ms":median(differences)})
                    }).collect();
                    groups.push(json!({"scenario":id,"workload_value":size,"fixture":fixture,"memory_profile_bytes":profile,
                        "completion_endpoint":spec["completion_endpoint"],"reference_gate_endpoint":budget["outcome"]["endpoint"],
                        "reference_gate":spec["gate_id"],
                        "budget_ms":limit,"complete":complete,"stable":stable,"classification":classification,
                        "rungs":RUNGS.iter().zip(medians).map(|(r,m)|json!({"rung":r,"median_ms":m})).collect::<Vec<_>>(),"contrasts":contrasts,
                        "memory":rows.iter().map(|t|memory_assessment(&t.memory,&manifest["durability_policy"])).collect::<Result<Vec<_>>>()?}));
                }
            }
        }
    }
    Ok(LadderArtifact {version:1,source_context:Value::Null,diagnostic_only:true,promise_eligible:false,complete:gaps.is_empty(),gaps,groups,trials,
        interpretation:"Paired diagnostic contrasts are not exclusive causal phase costs. Budget simulation does not constrain physical RAM. Instrument counts are companion signals; instrumented timings cannot establish latency gates.".into()})
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn fixture() -> (Value, Vec<Trial>) {
        let policy = json!({"version":1,"pending_queue_limit_bytes":33554432,"manifest_sync":"FULL","body_sync":"NORMAL","maximum_exposure_ms":null});
        let paths: BTreeMap<_, _> = RUNGS
            .iter()
            .map(|r| (r.to_string(), format!("matcher/{r}")))
            .collect();
        let manifest = json!({"durability_policy":policy,"capacity_budgets":{"search":{"threshold_ms":180}},
            "diagnostics":{"residency":{"version":1,"independent_launches":3,"memory_profiles":[4294967296u64,17179869184u64,null],
            "operations":{"search":{"gate_section":"capacity_budgets","gate_id":"search","workloads":[64],"workload_unit":"bytes","fixtures":["unicode_crlf"],"completion_endpoint":"all_ranges_returned","metric":"/elapsed_ns","code_paths":paths}}}}});
        let mut trials = Vec::new();
        for profile in [
            Some(4 * 1024 * 1024 * 1024u64),
            Some(16 * 1024 * 1024 * 1024u64),
            None,
        ] {
            for launch in 0..3 {
                for rung in RUNGS {
                    trials.push(Trial{scenario:"search".into(),workload_value:64,workload_unit:"bytes".into(),fixture:"unicode_crlf".into(),
                        fixture_digest:"a".repeat(64),result_digest:"b".repeat(64),result_count:1,expected_result_count:1,
                        code_path:format!("matcher/{rung}"),completion_endpoint:"all_ranges_returned".into(),revision:"fixture-revision".into(),rung:rung.into(),
                        memory_profile_bytes:profile,host_memory_bytes:32*1024*1024*1024,launch_index:launch,metric_ns:Some(1000000),metric:"/elapsed_ns".into(),timing_source:"native_wall".into(),
                        status:"ok".into(),unavailable_reason:None,cancellation_checked:true,
                        observations:json!({"store_read_count":if rung=="store_warm" {1} else {0},"retained_read_count":1,"app_cache_empty_at_start":true,"os_cache_primed":true,"eviction_succeeded":true,"physical_read_bytes":4096}),
                        memory:json!({"configured_pending_limit_bytes":33554432}),
                        measurement_quality:json!({"before":{"median_ns":100,"min_ns":100,"max_ns":100},"after":{"median_ns":100,"min_ns":100,"max_ns":100}}),instructions:BTreeMap::new()});
                }
            }
        }
        (manifest, trials)
    }
    #[test]
    fn complete_ladder_never_certifies_a_promise_and_missing_rungs_stay_missing() {
        let (m, mut t) = fixture();
        let a = analyze(&m, t.clone(), &[]).unwrap();
        assert!(a.complete);
        assert!(!a.promise_eligible);
        assert_eq!(
            a.groups[0]["classification"],
            "all_diagnostic_rungs_within_budget"
        );
        t.pop();
        let a = analyze(&m, t, &[]).unwrap();
        assert!(!a.complete);
        assert!(!a.gaps.is_empty());
    }
    #[test]
    fn mismatched_results_duplicate_trials_and_false_cold_claims_fail() {
        let (m, t) = fixture();
        let mut bad = t.clone();
        bad[1].result_digest = "c".repeat(64);
        assert!(analyze(&m, bad, &[]).is_err());
        let mut bad = t.clone();
        bad.push(t[0].clone());
        assert!(analyze(&m, bad, &[]).is_err());
        let mut bad = t.clone();
        bad[4].observations["physical_read_bytes"] = json!(0);
        assert!(analyze(&m, bad, &[]).is_err());
        let mut bad = t.clone();
        bad[0].timing_source = "callgrind_wall".into();
        assert!(analyze(&m, bad, &[]).is_err());
        assert!(analyze(&m, t, &["typo".into()]).is_err());
    }
    #[test]
    fn unavailable_rung_is_not_zero_latency_and_noise_is_not_a_classification() {
        let (m, mut t) = fixture();
        t[4].status = "unavailable".into();
        t[4].metric_ns = None;
        t[4].unavailable_reason = Some("eviction unsupported".into());
        assert!(!analyze(&m, t, &[]).unwrap().complete);
        let (m, mut t) = fixture();
        t[0].measurement_quality = json!({"status":"stable"});
        assert_eq!(
            analyze(&m, t, &[]).unwrap().groups[0]["classification"],
            "uncertain_noise"
        );
    }
    #[test]
    fn memory_retention_and_durability_policy_are_independent() {
        let (m, _) = fixture();
        let p = &m["durability_policy"];
        let a=memory_assessment(&json!({"resident_durable_bytes":1000,"resident_durable_peak_bytes":2000,"configured_pending_limit_bytes":33554432}),p).unwrap();
        assert_eq!(a["policy_change"], false);
        assert!(a["exposure_window_ms"].is_null());
        let a=memory_assessment(&json!({"configured_pending_limit_bytes":1073741824,"publication_timestamp_ns":1000000,"full_commit_timestamp_ns":5000000}),p).unwrap();
        assert_eq!(a["classification"], "durability_policy_change");
        assert_eq!(a["exposure_window_ms"], 4.0);
        assert!(memory_assessment(&json!({"scratch_bytes":10,"scratch_peak_bytes":1}), p).is_err());
        assert!(memory_assessment(
            &json!({"publication_timestamp_ns":10,"full_commit_timestamp_ns":1}),
            p
        )
        .is_err());
    }

    #[test]
    fn invalid_declarations_fail_before_any_probe_runs() {
        let (m, _) = fixture();
        for (pointer, value) in [
            ("/diagnostics/residency/version", json!(2)),
            (
                "/diagnostics/residency/memory_profiles",
                json!([4294967296u64, 17179869184u64, null, null]),
            ),
            (
                "/diagnostics/residency/operations/search/workloads",
                json!([]),
            ),
            (
                "/diagnostics/residency/operations/search/fixtures",
                json!([42]),
            ),
            (
                "/diagnostics/residency/operations/search/metric",
                json!("/wrong_ns"),
            ),
            (
                "/diagnostics/residency/operations/search/code_paths/store_resident",
                json!(""),
            ),
        ] {
            let mut bad = m.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(analyze(&bad, Vec::new(), &[]).is_err(), "{pointer}");
        }
    }
}
