//! Opt-in diagnostic contracts. No diagnostic row can establish a promise pass.
use anyhow::{ensure, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
mod plan;
pub(crate) use plan::{Plan, Request};

pub const RUNGS: [&str; 5] = [
    "owned_contiguous",
    "owned_piece_tree",
    "store_resident",
    "store_warm",
    "store_cold",
];

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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
        matches!(policy["version"].as_u64(), Some(1 | 2))
            && positive(&policy["pending_queue_limit_bytes"]).is_some(),
        "invalid durability policy"
    );
    if policy["version"] == 2 {
        let max = positive(&policy["pending_queue_limit_bytes"]).unwrap();
        let min = positive(&policy["minimum_pending_queue_bytes"])
            .context("missing minimum pending limit")?;
        ensure!(min <= max, "minimum pending limit exceeds maximum");
        ensure!(
            positive(&policy["available_memory_divisor"]).is_some(),
            "invalid memory divisor"
        );
        ensure!(
            policy["pressure_pending_limit_bytes"] == min,
            "unexpected pressure limit"
        );
        ensure!(
            policy["pending_limit_policy"] == "available_memory_clamped_failure_budget",
            "unknown admission policy"
        );
        ensure!(
            policy["memory_pressure_some_avg10_percent"]
                .as_f64()
                .is_some_and(|v| v.is_finite() && v > 0.0 && v <= 100.0),
            "invalid pressure threshold"
        );
    }
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
    let expected = if policy["version"] == 1 {
        policy["pending_queue_limit_bytes"].as_u64()
    } else {
        for key in [
            "policy_available_memory_bytes",
            "policy_failure_limit_bytes",
        ] {
            ensure!(
                memory[key].is_null() || positive(&memory[key]).is_some(),
                "invalid observation: {key}"
            );
        }
        ensure!(
            memory["policy_memory_pressure"].is_null()
                || memory["policy_memory_pressure"].is_boolean(),
            "invalid pressure observation"
        );
        positive(&memory["policy_available_memory_bytes"])
            .zip(positive(&memory["policy_failure_limit_bytes"]))
            .zip(memory["policy_memory_pressure"].as_bool())
            .map(|((available, failure), pressure)| {
                let min = policy["minimum_pending_queue_bytes"].as_u64().unwrap();
                let max = policy["pending_queue_limit_bytes"].as_u64().unwrap();
                let divisor = policy["available_memory_divisor"].as_u64().unwrap();
                (if pressure {
                    min
                } else {
                    (available / divisor).clamp(min, max)
                })
                .min(failure)
            })
    };
    let policy_change = configured.zip(expected).map(|(n, limit)| n != limit);
    Ok(json!({"metrics":memory,"exposure_window_ms":exposure,
        "policy_change":policy_change,
        "classification":match policy_change {Some(true)=>"durability_policy_change",Some(false)=>"declared_pending_policy",None=>"unknown_policy"},
        "exposure_limit_exceeded": exposure.zip(policy["maximum_exposure_ms"].as_f64()).map(|(a,b)| a>b),
        "missing_metrics":(["resident_durable_bytes","resident_durable_peak_bytes","accepted_not_durable_bytes","accepted_not_durable_peak_bytes","scratch_peak_bytes"].into_iter().filter(|k|memory[*k].is_null()).collect::<Vec<_>>()),
        "interpretation":"NORMAL body commit is separate from FULL durability; pending queue is not total accepted uncommitted text; unknown metrics are not zero"}))
}

fn validate_trial(t: &Trial, spec: &plan::Operation) -> Result<()> {
    ensure!(RUNGS.contains(&t.rung.as_str()), "unknown rung");
    ensure!(
        t.workload_value > 0 && !t.revision.is_empty() && !t.code_path.is_empty(),
        "missing trial identity"
    );
    ensure!(
        t.workload_unit == spec.workload_unit && t.completion_endpoint == spec.completion_endpoint,
        "workload unit or completion endpoint mismatch"
    );
    ensure!(
        t.timing_source == "native_wall",
        "instrumented timings cannot be used in residency latency comparisons"
    );
    ensure!(
        spec.code_paths.get(&t.rung) == Some(&t.code_path),
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
    ensure!(t.metric == spec.metric, "diagnostic metric mismatch");
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
        _ => anyhow::bail!("unknown rung"),
    }
    Ok(())
}

pub fn analyze(
    manifest: &Value,
    trials: Vec<Trial>,
    selected: &[String],
) -> Result<LadderArtifact> {
    Plan::new(manifest, selected)?.analyze(trials)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{analyze, memory_assessment, Plan, Trial, RUNGS};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
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
    fn ladder_classifies_only_complete_stable_comparable_trials() {
        for (case, expected) in [
            ("baseline", "all_diagnostic_rungs_within_budget"),
            ("owned_contiguous", "computation"),
            ("owned_piece_tree", "piece_tree_or_window_assembly"),
            ("store_resident", "resident_store_access"),
            ("store_warm", "retrieval_or_residency"),
            ("store_cold", "retrieval_or_residency"),
            ("missing", "incomplete"),
            ("unavailable", "incomplete"),
            ("noise", "uncertain_noise"),
        ] {
            let (m, mut t) = fixture();
            match case {
                "missing" => {
                    t.pop();
                }
                "unavailable" => {
                    t[4].status = "unavailable".into();
                    t[4].metric_ns = None;
                    t[4].unavailable_reason = Some("eviction unsupported".into());
                }
                "noise" => t[0].measurement_quality = json!({"status":"stable"}),
                _ => {
                    for row in t.iter_mut().filter(|t| t.rung == case) {
                        row.metric_ns = Some(200000000);
                    }
                }
            }
            let a = analyze(&m, t, &[]).unwrap();
            assert!(!a.promise_eligible);
            assert_eq!(a.complete, !matches!(case, "missing" | "unavailable"));
            let group = if case == "missing" {
                a.groups.last()
            } else {
                a.groups.first()
            }
            .unwrap();
            assert_eq!(group["classification"], expected, "{case}");
            if case == "owned_contiguous" {
                assert_eq!(group["contrasts"][0]["paired_median_difference_ms"], -199.0);
            }
        }
    }

    #[test]
    fn invalid_trial_identity_results_and_statistics_are_rejected() {
        for (index, pointer, value) in [
            (0, "/scenario", json!("typo")),
            (0, "/revision", json!("other")),
            (0, "/rung", json!("unknown")),
            (0, "/workload_value", json!(1)),
            (0, "/fixture", json!("other")),
            (0, "/fixture_digest", json!("bad")),
            (0, "/result_count", json!(2)),
            (1, "/result_digest", json!("c".repeat(64))),
            (0, "/workload_unit", json!("tabs")),
            (0, "/code_path", json!("fake")),
            (0, "/completion_endpoint", json!("dispatch")),
            (0, "/host_memory_bytes", json!(0)),
            (0, "/memory_profile_bytes", json!(1)),
            (0, "/launch_index", json!(3)),
            (0, "/timing_source", json!("callgrind_wall")),
            (0, "/cancellation_checked", json!(false)),
            (0, "/status", json!("unavailable")),
            (0, "/metric_ns", Value::Null),
            (0, "/metric", json!("/wrong_ns")),
            (0, "/observations/store_read_count", json!(1)),
            (2, "/observations/retained_read_count", json!(0)),
            (3, "/observations/os_cache_primed", json!(false)),
            (4, "/observations/physical_read_bytes", json!(0)),
        ] {
            let (m, mut t) = fixture();
            let mut raw = serde_json::to_value(&t[index]).unwrap();
            *raw.pointer_mut(pointer).unwrap() = value;
            t[index] = serde_json::from_value(raw).unwrap();
            assert!(analyze(&m, t, &[]).is_err(), "{index}:{pointer}");
        }
        let (mut m, mut t) = fixture();
        let duplicate = serde_json::from_value(serde_json::to_value(&t[0]).unwrap()).unwrap();
        t.push(duplicate);
        assert!(analyze(&m, t, &[]).is_err());
        let (_, mut t) = fixture();
        m["diagnostics"]["residency"]["operations"]["search"]["metric"] = json!("/p99_ns");
        m["capacity_budgets"]["search"]["metric"] = json!("/p99_ns");
        t[0].metric = "/p99_ns".into();
        t[0].metric_ns = Some(127);
        t[0].observations["metric_samples_ns"] = json!((1..=128).collect::<Vec<_>>());
        let plan = Plan::new(&m, &[]).unwrap();
        assert!(plan.validate_trial(&t[0]).is_ok());
        for bad in [128, 0] {
            t[0].metric_ns = Some(bad);
            assert!(plan.validate_trial(&t[0]).is_err());
        }
        assert!(analyze(&m, Vec::new(), &["typo".into()]).is_err());
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
    fn version_two_checks_the_formula_and_keeps_missing_inputs_unknown() {
        let fixture: Value =
            serde_json::from_str(include_str!("../tests/fixtures/outcomes-policy-v2.json"))
                .unwrap();
        let policy = &fixture["durability_policy"];
        let mut memory = json!({"configured_pending_limit_bytes":134217728,
            "policy_available_memory_bytes":4294967296u64,
            "policy_failure_limit_bytes":1073741824,
            "policy_memory_pressure":false});
        assert_eq!(
            memory_assessment(&memory, policy).unwrap()["policy_change"],
            false
        );
        memory["policy_memory_pressure"] = json!(true);
        assert_eq!(
            memory_assessment(&memory, policy).unwrap()["policy_change"],
            true
        );
        memory["configured_pending_limit_bytes"] = json!(33554432);
        assert_eq!(
            memory_assessment(&memory, policy).unwrap()["policy_change"],
            false
        );
        memory["policy_failure_limit_bytes"] = json!(16777216);
        memory["configured_pending_limit_bytes"] = json!(16777216);
        assert_eq!(
            memory_assessment(&memory, policy).unwrap()["policy_change"],
            false
        );
        memory["policy_available_memory_bytes"] = Value::Null;
        assert!(memory_assessment(&memory, policy).unwrap()["policy_change"].is_null());
        let mut bad = policy.clone();
        bad["minimum_pending_queue_bytes"] = json!(u64::MAX);
        assert!(super::validate_policy(&bad).is_err());
        bad = policy.clone();
        bad["available_memory_divisor"] = json!(0);
        assert!(super::validate_policy(&bad).is_err());
    }

    #[test]
    fn invalid_declarations_fail_before_any_probe_runs() {
        let (mut m, _) = fixture();
        m["capacity_budgets"]["search"]["metric"] = json!("/elapsed_ns");
        for (pointer, value) in [
            ("/capacity_budgets/search/metric", Value::Null),
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
