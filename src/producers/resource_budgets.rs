//! Project-owned limits apply to the exact resource operation, including first saves.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn attach(root: &Path, events: &mut [Value]) -> Result<()> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(
        root.join("benches/measurement_manifest.json"),
    )?)?;
    let budgets = manifest["resource_budgets"]
        .as_object()
        .context("missing project resource budgets")?;
    for event in events {
        let id = event["scenario"]
            .as_str()
            .context("missing resource scenario")?;
        let mut budget = budgets.get(id).cloned().unwrap_or(Value::Null);
        if let Some(object) = budget.as_object_mut() {
            object.insert("authoritative".into(), json!(true));
            object.insert("source".into(), json!("benches/measurement_manifest.json"));
        }
        event["resource_budget"] = budget;
        event["budget_status"] = json!(status(event));
    }
    Ok(())
}

pub(super) fn status(sample: &Value) -> &'static str {
    let budget = &sample["resource_budget"];
    if budget["authoritative"] != true {
        return "unbudgeted";
    }
    let Some(metric) = budget["metric"]
        .as_str()
        .and_then(|path| sample.pointer(path))
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
    else {
        return "invalid";
    };
    let Some(limit) = budget["max"]
        .as_f64()
        .filter(|limit| limit.is_finite() && *limit > 0.0)
    else {
        return "invalid";
    };
    if metric > limit {
        "fail"
    } else {
        "pass"
    }
}

/// Preserve uncertainty when independent observations straddle the limit.
pub(super) fn group_status(samples: &[Value]) -> &'static str {
    let mut groups: std::collections::BTreeMap<String, Vec<&Value>> =
        std::collections::BTreeMap::new();
    for sample in samples {
        groups
            .entry(sample["workload_value"].to_string())
            .or_default()
            .push(sample);
    }
    if groups.is_empty() {
        return "unmeasured";
    }
    let mut uncertain = false;
    for group in groups.values() {
        if group
            .iter()
            .any(|sample| matches!(status(sample), "invalid" | "unbudgeted"))
        {
            return "unbudgeted";
        }
        let budget = &group[0]["resource_budget"];
        if group
            .iter()
            .any(|sample| &sample["resource_budget"] != budget)
        {
            return "invalid";
        }
        let metric = budget["metric"].as_str().unwrap();
        let limit = budget["max"].as_f64().unwrap();
        let mut values: Vec<_> = group
            .iter()
            .map(|sample| sample.pointer(metric).and_then(Value::as_f64).unwrap())
            .collect();
        values.sort_by(f64::total_cmp);
        let median = (values[values.len() / 2] + values[(values.len() - 1) / 2]) / 2.0;
        if median > limit {
            return "fail";
        }
        uncertain |= values.last().is_some_and(|v| *v > limit);
    }
    if uncertain {
        "uncertain"
    } else {
        "pass"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_borderline_work_keeps_uncertainty_and_never_selects_the_fastest_sample() {
        let sample = |n| {
            json!({"workload_value":10000,"elapsed_ns":n,
            "resource_budget":{"metric":"/elapsed_ns","max":100,"authoritative":true}})
        };
        assert_eq!(
            super::group_status(&[sample(90), sample(95), sample(110)]),
            "uncertain"
        );
        assert_eq!(
            super::group_status(&[sample(90), sample(110), sample(120)]),
            "fail"
        );
        assert_eq!(
            super::group_status(&[sample(80), sample(90), sample(95)]),
            "pass"
        );
    }
    #[test]
    fn first_save_cannot_pass_on_workload_alone_or_on_another_operations_budget() {
        let mut sample = json!({"elapsed_ns":5_200_000_000u64,"phase_elapsed_ns":{"save_ns":10}});
        assert_eq!(status(&sample), "unbudgeted");
        sample["resource_budget"] =
            json!({"metric":"/elapsed_ns","max":180_000_000,"authoritative":true});
        assert_eq!(status(&sample), "fail");
        sample["elapsed_ns"] = json!(100_000_000);
        assert_eq!(status(&sample), "pass");
        sample["resource_budget"]["metric"] = json!("/missing");
        assert_eq!(status(&sample), "invalid");
    }
}
