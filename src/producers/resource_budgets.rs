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
        event["fixture_contract"] = manifest["fixtures"][id].clone();
        event["resource_budget"] = budget;
        event["budget_status"] = json!(status(event));
    }
    Ok(())
}

fn observation(sample: &Value) -> Option<(f64, f64)> {
    let budget = &sample["resource_budget"];
    let value = sample
        .pointer(budget["metric"].as_str()?)?
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)?;
    let limit = budget["max"]
        .as_f64()
        .filter(|limit| limit.is_finite() && *limit > 0.0)?;
    Some((value, limit))
}

pub(super) fn status(sample: &Value) -> &'static str {
    if sample["resource_budget"]["authoritative"] != true {
        return "unbudgeted";
    }
    match observation(sample) {
        Some((value, limit)) if value > limit => "fail",
        Some(_) => "pass",
        None => "invalid",
    }
}

/// Preserve uncertainty when independent observations straddle the limit.
pub(super) fn group_status<'a>(samples: impl IntoIterator<Item = &'a Value>) -> &'static str {
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
    match groups
        .values()
        .try_fold(false, |uncertain, group| match workload_status(group) {
            "pass" => Ok(uncertain),
            "uncertain" => Ok(true),
            failure => Err(failure),
        }) {
        Ok(true) => "uncertain",
        Ok(false) => "pass",
        Err(failure) => failure,
    }
}

fn workload_status(group: &[&Value]) -> &'static str {
    let parsed: Option<Vec<_>> = group
        .iter()
        .filter(|sample| sample["resource_budget"]["authoritative"] == true)
        .map(|sample| observation(sample))
        .collect();
    let Some(parsed) = parsed.filter(|parsed| parsed.len() == group.len()) else {
        return "unbudgeted";
    };
    let Some(first) = group.first() else {
        return "unmeasured";
    };
    if group
        .iter()
        .any(|sample| sample["resource_budget"] != first["resource_budget"])
    {
        return "invalid";
    }
    let mut values: Vec<_> = parsed.iter().map(|(value, _)| *value).collect();
    values.sort_by(f64::total_cmp);
    let limit = parsed[0].1;
    let median = (values[values.len() / 2] + values[(values.len() - 1) / 2]) / 2.0;
    if median > limit {
        "fail"
    } else if values.last().is_some_and(|value| *value > limit) {
        "uncertain"
    } else {
        "pass"
    }
}

#[cfg(test)]
mod tests {
    use super::{group_status, status};
    use serde_json::{json, Value};
    #[test]
    fn budgets_keep_uncertainty_and_cannot_select_the_fastest_observation() {
        let sample = |n| json!({"workload_value":10000,"elapsed_ns":n,"resource_budget":{"metric":"/elapsed_ns","max":100,"authoritative":true}});
        for (times, expected) in [
            ([90, 95, 110], "uncertain"),
            ([90, 110, 120], "fail"),
            ([80, 90, 95], "pass"),
        ] {
            assert_eq!(
                group_status(times.iter().map(|n| sample(*n)).collect::<Vec<_>>().iter()),
                expected
            );
        }
        let mut row = sample(5200);
        assert_eq!(status(&row), "fail");
        row["elapsed_ns"] = json!(10);
        assert_eq!(status(&row), "pass");
        row["resource_budget"]["metric"] = json!("/missing");
        assert_eq!(status(&row), "invalid");
        row["resource_budget"] = Value::Null;
        assert_eq!(status(&row), "unbudgeted");
    }
}
