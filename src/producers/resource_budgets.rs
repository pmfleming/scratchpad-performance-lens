//! Project-owned limits apply to the exact resource operation, including first saves.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn attach(root: &Path, events: &mut [Value]) -> Result<()> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(
        root.join("benches/measurement_manifest.json"),
    )?)?;
    if manifest.get("outcome_contract_version").is_some() {
        crate::outcomes::validate(&manifest)?;
    }
    let budgets = manifest["resource_budgets"]
        .as_object()
        .context("missing project resource budgets")?;
    for event in events {
        let id = event["scenario"]
            .as_str()
            .context("missing resource scenario")?
            .to_owned();
        let mut budget = budgets.get(&id).cloned().unwrap_or(Value::Null);
        if let Some(object) = budget.as_object_mut() {
            object.insert("authoritative".into(), json!(true));
            object.insert("source".into(), json!("benches/measurement_manifest.json"));
        }
        event["fixture_contract"] = manifest["fixtures"][&id].clone();
        event["resource_budget"] = budget;
        crate::outcomes::annotate(&manifest, "resource_budgets", &id, event);
        if manifest.get("durability_policy").is_some() {
            event["memory_assessment"] = crate::residency::memory_assessment(
                &event["memory_observation"],
                &manifest["durability_policy"],
            )?;
        }
        event["budget_status"] = json!(status(event));
    }
    Ok(())
}

fn observation(sample: &Value) -> Option<(f64, f64)> {
    if sample["status"] != "ok" || sample["outcome_valid"] == false {
        return None;
    }
    let budget = &sample["resource_budget"];
    if budget["measurement_scope"].is_string()
        && sample["measurement_scope"] != budget["measurement_scope"]
    {
        return None;
    }
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
    if group
        .iter()
        .any(|sample| sample["resource_budget"]["authoritative"] != true)
    {
        return "unbudgeted";
    }
    let parsed: Option<Vec<_>> = group.iter().map(|sample| observation(sample)).collect();
    let Some(parsed) = parsed.filter(|parsed| parsed.len() == group.len()) else {
        return "invalid";
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
    if !complete_launches(group) {
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

fn complete_launches(group: &[&Value]) -> bool {
    let Some(first) = group.first() else {
        return false;
    };
    let minimum = match first["resource_budget"].get("min_independent_launches") {
        Some(value) => match value.as_u64().filter(|count| (1..=16).contains(count)) {
            Some(count) => count,
            None => return false,
        },
        None => 1,
    };
    if group.iter().all(|sample| {
        sample.get("independent_launch_count").is_none() && sample.get("launch_index").is_none()
    }) {
        return minimum == 1;
    }
    let Some(expected) = first["independent_launch_count"]
        .as_u64()
        .filter(|count| *count >= minimum)
    else {
        return false;
    };
    let mut observed = std::collections::BTreeSet::new();
    for sample in group {
        if sample["independent_launch_count"].as_u64() != Some(expected) {
            return false;
        }
        let Some(index) = sample["launch_index"]
            .as_u64()
            .filter(|index| *index < expected)
        else {
            return false;
        };
        observed.insert(index);
    }
    observed.len() as u64 == expected
}

#[cfg(test)]
mod tests {
    use super::{group_status, status};
    use serde_json::{json, Value};
    #[test]
    fn missing_or_duplicate_launches_cannot_prove_a_repeated_workload() {
        let rows: Vec<_> = (0..3).map(|index| json!({"status":"ok","workload_value":100,
            "elapsed_ns":80,"launch_index":index,"independent_launch_count":3,
            "resource_budget":{"metric":"/elapsed_ns","max":100,"authoritative":true,"min_independent_launches":3}})).collect();
        assert_eq!(group_status(&rows), "pass");
        assert_eq!(group_status(&rows[..2]), "invalid");
        let mut duplicated = rows.clone();
        duplicated[2]["launch_index"] = json!(0);
        assert_eq!(group_status(&duplicated), "invalid");
        let mut inconsistent = rows.clone();
        inconsistent[2]["independent_launch_count"] = json!(1);
        assert_eq!(group_status(&inconsistent), "invalid");
        let mut missing = rows[0].clone();
        missing.as_object_mut().unwrap().remove("launch_index");
        missing
            .as_object_mut()
            .unwrap()
            .remove("independent_launch_count");
        assert_eq!(group_status([&missing]), "invalid");
    }
    #[test]
    fn fixture_inclusive_or_missing_scope_cannot_satisfy_an_operation_budget() {
        let mut sample = json!({"status":"ok","elapsed_ns":10,"resource_budget":{
            "metric":"/elapsed_ns","max":100,"authoritative":true,
            "measurement_scope":"prepared_operation"}});
        assert_eq!(status(&sample), "invalid");
        sample["measurement_scope"] = json!("scenario_including_fixture_work");
        assert_eq!(status(&sample), "invalid");
        sample["measurement_scope"] = json!("prepared_operation");
        assert_eq!(status(&sample), "pass");
        sample["elapsed_ns"] = json!(110);
        assert_eq!(status(&sample), "fail");
    }
    #[test]
    fn budgets_keep_uncertainty_and_cannot_select_the_fastest_observation() {
        let sample = |n| json!({"status":"ok","workload_value":10000,"elapsed_ns":n,"resource_budget":{"metric":"/elapsed_ns","max":100,"authoritative":true}});
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
        for failed in [json!("panic"), json!("failed"), Value::Null] {
            row["status"] = failed;
            assert_eq!(status(&row), "invalid");
            assert_eq!(group_status([&row]), "invalid");
        }
        row["status"] = json!("ok");
        row["resource_budget"]["metric"] = json!("/missing");
        assert_eq!(status(&row), "invalid");
        row["resource_budget"] = Value::Null;
        assert_eq!(status(&row), "unbudgeted");
    }
}
