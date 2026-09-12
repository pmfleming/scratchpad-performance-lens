use super::health::coverage_axis;
use super::registry::ReviewScenario;
use crate::artifacts::capacity_report::CapacityScenario;
use crate::artifacts::performance_review::CoverageAxes;
use serde_json::Value;

pub(super) fn valid_latency(row: &Value) -> bool {
    if row["evidence_kind"] == "frame" { return super::frames::valid_frame(row); }
    let measured = super::super::common::budget_probe_ms(row);
    measured.is_some_and(|value| value.is_finite() && value > 0.0)
        && row["threshold_ms"].as_f64().is_some_and(|value| value > 0.0)
        && row["threshold_authoritative"].as_bool() == Some(true)
        && row["measurement_scope"].as_str().is_some_and(|scope| !scope.is_empty() && scope != "unknown")
        && row["measurement_status"].as_str() == Some("valid")
        && matches!(row["budget_status"].as_str(), Some("pass" | "fail"))
}

pub(super) fn valid_resource(row: &Value) -> bool {
    row["samples"].as_array().is_some_and(|samples| !samples.is_empty() && samples.iter().all(|sample| {
        sample["status"].as_str().is_some_and(|status| matches!(status, "ok" | "panic"))
            && sample["elapsed_ms"].as_f64().is_some_and(|value| value > 0.0)
            && sample["measurement_scope"].as_str().is_some_and(|scope| scope != "unknown" && !scope.is_empty())
            && ["allocated_bytes", "peak_live_bytes", "working_set_bytes"].iter()
                .any(|key| sample[*key].as_u64().is_some())
    }))
}

pub(super) fn axes(scenario: &ReviewScenario, latency: &[Value], capacity: &[CapacityScenario],
    resources: &[Value], profiles: &[Value], capacity_synthetic: bool, resources_synthetic: bool) -> CoverageAxes {
    let latency_id = |row: &Value| row["benchmark_key"].as_str().unwrap_or("").to_owned();
    let resource_id = |row: &Value| row["scenario"].as_str().unwrap_or("").to_owned();
    let profile_id = |row: &Value| row["id"].as_str().unwrap_or("").to_owned();
    CoverageAxes {
        speed: coverage_axis(scenario.benchmark_keys, latency.iter().map(latency_id).collect(),
            latency.iter().filter(|row| valid_latency(row)).map(latency_id).collect()),
        capacity: coverage_axis(scenario.capacity_scenarios, capacity.iter().map(|row| row.scenario.clone()).collect(),
            capacity.iter().filter(|row| !capacity_synthetic && row.threshold_ms > 0.0 && !row.samples.is_empty()
                && row.samples.iter().all(|sample| sample.elapsed_ms > 0.0 && sample.repetition_count > 0
                    && sample.measurement_scope.as_deref().is_some_and(|scope| scope != "unknown")
                    && matches!(sample.status.as_str(), "ok" | "panic")))
                .map(|row| row.scenario.clone()).collect()),
        resource: coverage_axis(scenario.resource_scenarios, resources.iter().map(resource_id).collect(),
            resources.iter().filter(|row| !resources_synthetic && valid_resource(row)).map(resource_id).collect()),
        // Profiles explain measured problems; they do not prove or gate a promise.
        profiles: coverage_axis(&[], profiles.iter().map(profile_id).collect(), profiles.iter()
            .filter(|row| row["available"].as_bool() == Some(true)).map(profile_id).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn required_operations_cannot_be_satisfied_by_a_sibling_or_unbudgeted_row() {
        let axis = coverage_axis(&["open", "scroll"], vec!["open".into()], vec!["open".into()]);
        assert!(!axis.covered);
        assert_eq!(axis.missing_evidence, ["scroll"]);
        assert!(!valid_latency(&json!({"mean_ms": 1, "threshold_ms": null})));
    }
    #[test]
    fn unavailable_profiles_are_not_valid_evidence_or_required() {
        let axis = coverage_axis(&[], vec!["profile".into()], vec![]);
        assert!(!axis.required);
        assert!(!axis.covered);
        assert_eq!(axis.valid_count, 0);
    }
}
