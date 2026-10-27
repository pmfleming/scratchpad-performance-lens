use anyhow::{ensure, Context, Result};
use clap::Args;
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path, path::PathBuf};

const SESSION: &str = "Session Persistence Restore";
const BENCHMARKS: &[&str] = &[
    "session_persist_latency/10000",
    "session_restore_latency/10000",
    "session_restore_background_completion/10000",
];

#[derive(Debug, Args)]
pub(crate) struct CompareSessionBaselineArgs {
    /// Archived run directory containing performance/performance_review.json.
    baseline: PathBuf,
    current: PathBuf,
    #[arg(long)]
    require_all_promises: bool,
}

pub(crate) fn compare_session_baseline(args: CompareSessionBaselineArgs) -> Result<()> {
    let baseline = read_run(&args.baseline)?;
    let current = read_run(&args.current)?;
    let (report, accepted) = compare(&baseline, &current, args.require_all_promises)?;
    println!("{report}");
    ensure!(accepted, "session baseline acceptance failed");
    Ok(())
}

fn read_run(path: &Path) -> Result<Value> {
    // Historical archives predate the combined quality/performance wrapper.
    let check = path.join("check.exit");
    let marker = if check.try_exists()? {
        check
    } else {
        path.join("performance.exit")
    };
    let status = fs::read_to_string(&marker)
        .with_context(|| format!("read run completion marker {}", marker.display()))?;
    ensure!(
        status.trim() == "0",
        "incomplete/failed lens run: {}",
        path.display()
    );
    let report = path.join("performance/performance_review.json");
    serde_json::from_slice(&fs::read(&report).with_context(|| report.display().to_string())?)
        .with_context(|| format!("parse {}", report.display()))
}

fn index<'a>(rows: &'a Value, key: &str) -> Result<BTreeMap<&'a str, &'a Value>> {
    let rows = rows.as_array().context("missing report rows")?;
    ensure!(!rows.is_empty(), "empty report rows");
    let mut indexed = BTreeMap::new();
    for row in rows {
        let name = row[key]
            .as_str()
            .with_context(|| format!("missing {key}"))?;
        ensure!(
            indexed.insert(name, row).is_none(),
            "duplicate {key}: {name}"
        );
    }
    Ok(indexed)
}

fn positive(value: &Value) -> Result<f64> {
    let value = value.as_f64().context("missing/nonnumeric latency")?;
    ensure!(
        value.is_finite() && value > 0.0,
        "latency must be finite and positive"
    );
    Ok(value)
}

fn compare(baseline: &Value, current: &Value, require_all: bool) -> Result<(String, bool)> {
    let before = index(&baseline["scenarios"], "title").context("baseline scenarios")?;
    let after = index(&current["scenarios"], "title").context("current scenarios")?;
    // A disappearing promise must not make --require-all-promises pass.
    ensure!(
        before.keys().eq(after.keys()),
        "baseline/current scenario sets differ"
    );
    let session_rows = |scenarios: &BTreeMap<&str, &Value>| -> Result<Value> {
        Ok(scenarios
            .get(SESSION)
            .context("missing Session Persistence Restore scenario")?["evidence"]["latency"]
            .clone())
    };
    let old_rows = session_rows(&before)?;
    let new_rows = session_rows(&after)?;
    let old_rows = index(&old_rows, "name").context("baseline session latency")?;
    let new_rows = index(&new_rows, "name").context("current session latency")?;
    let mut lines = Vec::new();
    let mut session_pass = true;
    for &name in BENCHMARKS {
        let old = old_rows
            .get(name)
            .with_context(|| format!("baseline missing {name}"))?;
        let new = new_rows
            .get(name)
            .with_context(|| format!("current missing {name}"))?;
        let old_mean =
            positive(&old["mean_ns"]).with_context(|| format!("baseline {name} mean"))?;
        let new_mean = positive(&new["mean_ns"]).with_context(|| format!("current {name} mean"))?;
        let old_upper = positive(&old["mean_confidence_interval_ns"]["upper_bound"])?;
        let new_upper = positive(&new["mean_confidence_interval_ns"]["upper_bound"])?;
        let authoritative = new["threshold_authoritative"]
            .as_bool()
            .with_context(|| format!("{name}: missing threshold authority"))?;
        let passed = new["measurement_status"] == "valid"
            && (!authoritative || new["budget_status"] == "pass")
            && new_mean <= old_mean
            && new_upper <= old_upper;
        session_pass &= passed;
        lines.push(format!(
            "{} {name}: {:.3} -> {:.3} ms ({:.3}x baseline)",
            if passed { "PASS" } else { "FAIL" },
            old_mean / 1e6,
            new_mean / 1e6,
            new_mean / old_mean,
        ));
    }
    let mut all_pass = true;
    for (title, scenario) in after {
        let old_health = before[title]["promise_health"]
            .as_str()
            .context("missing baseline promise health")?;
        let health = scenario["promise_health"]
            .as_str()
            .context("missing current promise health")?;
        all_pass &= health == "pass";
        lines.push(format!("PROMISE {title}: {old_health} -> {health}"));
    }
    lines.push(format!(
        "Session latency acceptance: {}",
        if session_pass { "PASS" } else { "FAIL" }
    ));
    lines.push(format!(
        "All-project promise compliance: {}",
        if all_pass { "PASS" } else { "NOT MET" }
    ));
    Ok((lines.join("\n"), session_pass && (!require_all || all_pass)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn review() -> Value {
        json!({"scenarios": [{"title": SESSION, "promise_health": "pass", "evidence": {
            "latency": BENCHMARKS.iter().map(|name| json!({
                "name": name, "measurement_status": "valid", "threshold_authoritative": true,
                "budget_status": "pass", "mean_ns": 100.0,
                "mean_confidence_interval_ns": {"upper_bound": 110.0}
            })).collect::<Vec<_>>()
        }}, {"title": "Other", "promise_health": "pass"}]})
    }

    #[test]
    fn equal_and_improved_measurements_pass() {
        let baseline = review();
        assert!(compare(&baseline, &baseline, true).unwrap().1);
        let mut current = review();
        current["scenarios"][0]["evidence"]["latency"][0]["mean_ns"] = json!(90);
        assert!(compare(&baseline, &current, true).unwrap().1);
    }

    #[test]
    fn regression_invalid_measurement_and_authoritative_budget_fail() {
        for (field, value) in [
            ("mean_ns", json!(101)),
            ("mean_confidence_interval_ns", json!({"upper_bound": 111})),
            ("measurement_status", json!("invalid")),
            ("budget_status", json!("uncertain")),
        ] {
            let mut current = review();
            current["scenarios"][0]["evidence"]["latency"][0][field] = value;
            assert!(!compare(&review(), &current, false).unwrap().1);
        }
    }

    #[test]
    fn nonauthoritative_budgets_and_optional_all_promise_gate_match_legacy_policy() {
        let mut current = review();
        let row = &mut current["scenarios"][0]["evidence"]["latency"][0];
        row["threshold_authoritative"] = json!(false);
        row["budget_status"] = json!("unmeasured");
        current["scenarios"][1]["promise_health"] = json!("at_risk");
        assert!(compare(&review(), &current, false).unwrap().1);
        assert!(!compare(&review(), &current, true).unwrap().1);
    }

    #[test]
    fn incomplete_duplicate_or_nonnumeric_evidence_is_rejected() {
        for invalid in [Value::Null, json!(-1), json!(0), json!("100")] {
            let mut current = review();
            current["scenarios"][0]["evidence"]["latency"][0]["mean_ns"] = invalid;
            assert!(compare(&review(), &current, false).is_err());
            assert!(compare(&current, &review(), false).is_err());
        }
        let mut missing = review();
        missing["scenarios"][0]["evidence"]["latency"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(compare(&review(), &missing, false).is_err());
        let mut duplicate = review();
        let row = duplicate["scenarios"][0]["evidence"]["latency"][0].clone();
        duplicate["scenarios"][0]["evidence"]["latency"]
            .as_array_mut()
            .unwrap()
            .push(row);
        assert!(compare(&review(), &duplicate, false).is_err());
        let mut missing_scenario = review();
        missing_scenario["scenarios"].as_array_mut().unwrap().pop();
        assert!(compare(&review(), &missing_scenario, true).is_err());
    }

    #[test]
    fn archive_markers_support_legacy_but_never_mask_a_failed_combined_run() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("performance")).unwrap();
        fs::write(
            dir.path().join("performance/performance_review.json"),
            review().to_string(),
        )
        .unwrap();
        assert!(read_run(dir.path()).is_err());
        fs::write(dir.path().join("performance.exit"), "0\n").unwrap();
        assert!(read_run(dir.path()).is_ok());
        fs::write(dir.path().join("check.exit"), "1\n").unwrap();
        assert!(read_run(dir.path()).is_err());
        fs::write(dir.path().join("check.exit"), "0\n").unwrap();
        assert!(read_run(dir.path()).is_ok());
        fs::write(dir.path().join("performance/performance_review.json"), "{").unwrap();
        assert!(read_run(dir.path()).is_err());
    }
}
