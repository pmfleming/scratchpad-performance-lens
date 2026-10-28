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

fn session_rows<'a>(scenarios: &BTreeMap<&str, &'a Value>) -> Result<BTreeMap<&'a str, &'a Value>> {
    index(
        &scenarios
            .get(SESSION)
            .context("missing Session Persistence Restore scenario")?["evidence"]["latency"],
        "name",
    )
}

fn compare(baseline: &Value, current: &Value, require_all: bool) -> Result<(String, bool)> {
    let before = index(&baseline["scenarios"], "title").context("baseline scenarios")?;
    let after = index(&current["scenarios"], "title").context("current scenarios")?;
    // A disappearing promise must not make --require-all-promises pass.
    ensure!(
        before.keys().eq(after.keys()),
        "baseline/current scenario sets differ"
    );
    let old_rows = session_rows(&before).context("baseline session latency")?;
    let new_rows = session_rows(&after).context("current session latency")?;
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
    use super::{compare, read_run, BENCHMARKS, SESSION};
    use serde_json::json;
    use serde_json::Value;
    use std::fs;

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
    fn acceptance_matrix_preserves_latency_budget_and_promise_policy() -> anyhow::Result<()> {
        for (field, value, accepted) in [
            ("mean_ns", json!(100), true),
            ("mean_ns", json!(90), true),
            ("mean_ns", json!(101), false),
            (
                "mean_confidence_interval_ns",
                json!({"upper_bound": 111}),
                false,
            ),
            ("measurement_status", json!("invalid"), false),
            ("budget_status", json!("uncertain"), false),
        ] {
            let mut current = review();
            current["scenarios"][0]["evidence"]["latency"][0][field] = value;
            assert_eq!(compare(&review(), &current, true)?.1, accepted, "{field}");
        }
        let mut current = review();
        let row = &mut current["scenarios"][0]["evidence"]["latency"][0];
        row["threshold_authoritative"] = json!(false);
        row["budget_status"] = json!("unmeasured");
        current["scenarios"][1]["promise_health"] = json!("at_risk");
        assert!(compare(&review(), &current, false)?.1);
        assert!(!compare(&review(), &current, true)?.1);
        Ok(())
    }

    #[test]
    fn incomplete_duplicate_or_nonnumeric_evidence_is_rejected() {
        for invalid in [Value::Null, json!(-1), json!(0), json!("100")] {
            let mut current = review();
            current["scenarios"][0]["evidence"]["latency"][0]["mean_ns"] = invalid;
            assert!(compare(&review(), &current, false).is_err());
            assert!(compare(&current, &review(), false).is_err());
        }
        let baseline = review();
        for (pointer, value) in [
            (
                "/scenarios/0/evidence/latency",
                json!([baseline["scenarios"][0]["evidence"]["latency"][0]]),
            ),
            ("/scenarios/0/evidence/latency/1/name", json!(BENCHMARKS[0])),
            ("/scenarios", json!([baseline["scenarios"][0]])),
        ] {
            let mut current = review();
            *current.pointer_mut(pointer).unwrap() = value;
            assert!(compare(&baseline, &current, true).is_err(), "{pointer}");
            assert!(compare(&current, &baseline, true).is_err(), "{pointer}");
        }
    }

    #[test]
    fn archive_markers_support_legacy_but_never_mask_a_failed_combined_run() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir(dir.path().join("performance"))?;
        let report = dir.path().join("performance/performance_review.json");
        fs::write(&report, review().to_string())?;
        assert!(read_run(dir.path()).is_err());
        for (marker, status, accepted) in [
            ("performance.exit", "0\n", true),
            ("check.exit", "1\n", false),
            ("check.exit", "0\n", true),
        ] {
            fs::write(dir.path().join(marker), status)?;
            assert_eq!(read_run(dir.path()).is_ok(), accepted);
        }
        fs::write(&report, "{")?;
        assert!(read_run(dir.path()).is_err());
        Ok(())
    }
}
