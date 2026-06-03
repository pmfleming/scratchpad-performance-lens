use super::registry::ReviewScenario;
use crate::artifacts::performance_review::{
    CeilingStatus, CoverageAxes, CoverageAxis, CoverageStatus, PromiseHealth, ScaleCheck,
    ScaleCheckStatus,
};
use serde_json::Value;
pub(super) fn coverage_axis(rows: &[Value], required: bool) -> CoverageAxis {
    CoverageAxis {
        required,
        covered: !rows.is_empty(),
        count: rows.len(),
    }
}

pub(super) fn scenario_gaps(axes: &CoverageAxes) -> Vec<String> {
    [
        ("speed", &axes.speed),
        ("capacity", &axes.capacity),
        ("resource", &axes.resource),
        ("profiles", &axes.profiles),
    ]
    .into_iter()
    .filter(|(_, axis)| axis.required && !axis.covered)
    .map(|(name, _)| format!("Missing {name} evidence."))
    .collect()
}

pub(super) fn scale_checks(
    scenario: &ReviewScenario,
    capacity_rows: &[Value],
    resource_rows: &[Value],
    capacity_synthetic: bool,
    resources_synthetic: bool,
) -> Vec<ScaleCheck> {
    scenario
        .scale_targets
        .iter()
        .map(|target| {
            let matching_capacity: Vec<_> = capacity_rows
                .iter()
                .filter(|row| row_scenario_matches(row, target.capacity_scenarios))
                .collect();
            let matching_resources: Vec<_> = resource_rows
                .iter()
                .filter(|row| row_scenario_matches(row, target.resource_scenarios))
                .collect();
            let max_workload_value = matching_capacity
                .iter()
                .chain(matching_resources.iter())
                .filter_map(|row| max_workload_value(row))
                .max();
            let first_failure_workload = matching_capacity
                .iter()
                .filter_map(|row| row.get("first_failure_workload").and_then(Value::as_i64))
                .min();
            let last_successful_workload = matching_capacity
                .iter()
                .filter_map(|row| row.get("last_successful_workload").and_then(Value::as_i64))
                .max();
            let synthetic = (capacity_synthetic && !matching_capacity.is_empty())
                || (resources_synthetic && !matching_resources.is_empty());
            let failed_before_promise = ceiling_failed_before_promise(
                target.minimum,
                first_failure_workload,
                last_successful_workload,
            );
            let met = !synthetic
                && !failed_before_promise
                && max_workload_value.is_some_and(|value| value >= target.minimum);
            let ceiling_status = ceiling_status(
                synthetic,
                first_failure_workload,
                last_successful_workload,
                target.minimum,
            );
            ScaleCheck {
                id: target.id.to_string(),
                label: target.label.to_string(),
                minimum: target.minimum,
                unit: target.unit.to_string(),
                max_workload_value,
                first_failure_workload,
                last_successful_workload,
                headroom_workload: headroom_workload(
                    target.minimum,
                    first_failure_workload,
                    last_successful_workload,
                ),
                evidence_count: matching_capacity.len() + matching_resources.len(),
                synthetic,
                met,
                ceiling_status,
                status: if synthetic {
                    ScaleCheckStatus::Unmeasured
                } else if failed_before_promise {
                    ScaleCheckStatus::FailedBeforePromise
                } else if met {
                    ScaleCheckStatus::Met
                } else {
                    ScaleCheckStatus::Missing
                },
            }
        })
        .collect()
}

fn ceiling_failed_before_promise(
    minimum: i64,
    first_failure_workload: Option<i64>,
    last_successful_workload: Option<i64>,
) -> bool {
    first_failure_workload.is_some_and(|failure| {
        failure < minimum
            || (failure == minimum
                && last_successful_workload.is_none_or(|success| success < minimum))
    })
}

fn ceiling_status(
    synthetic: bool,
    first_failure_workload: Option<i64>,
    last_successful_workload: Option<i64>,
    minimum: i64,
) -> CeilingStatus {
    if synthetic {
        CeilingStatus::Unmeasured
    } else if ceiling_failed_before_promise(
        minimum,
        first_failure_workload,
        last_successful_workload,
    ) {
        CeilingStatus::FailedBeforePromise
    } else if first_failure_workload.is_some() {
        CeilingStatus::FailedAfterPromise
    } else {
        CeilingStatus::NotReached
    }
}

fn headroom_workload(
    minimum: i64,
    first_failure_workload: Option<i64>,
    last_successful_workload: Option<i64>,
) -> Option<i64> {
    first_failure_workload
        .filter(|failure| *failure > minimum)
        .or_else(|| last_successful_workload.filter(|success| *success >= minimum))
        .map(|value| value - minimum)
}

fn row_scenario_matches(row: &Value, scenario_ids: &[&str]) -> bool {
    row.get("scenario")
        .and_then(Value::as_str)
        .is_some_and(|id| scenario_ids.contains(&id))
}

fn max_workload_value(row: &Value) -> Option<i64> {
    let direct = [
        "last_successful_workload",
        "first_failure_workload",
        "workload_value",
        "result_value",
    ]
    .into_iter()
    .filter_map(|key| row.get(key).and_then(Value::as_i64));
    let samples = row
        .get("samples")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|sample| {
            sample
                .get("workload_value")
                .or_else(|| sample.get("result_value"))
                .and_then(Value::as_i64)
        });
    direct.chain(samples).max()
}

pub(super) fn promise_health(
    coverage_status: CoverageStatus,
    budget_misses: usize,
    scale_failures: usize,
    missing_scale_targets: usize,
    synthetic_evidence: bool,
) -> PromiseHealth {
    if synthetic_evidence || coverage_status == CoverageStatus::Missing {
        PromiseHealth::Unmeasured
    } else if budget_misses > 0 || scale_failures > 0 {
        PromiseHealth::Fail
    } else if coverage_status == CoverageStatus::Thin || missing_scale_targets > 0 {
        PromiseHealth::AtRisk
    } else {
        PromiseHealth::Pass
    }
}

pub(super) fn scenario_opportunities(
    promise_health: PromiseHealth,
    gaps_empty: bool,
) -> Vec<String> {
    match promise_health {
        PromiseHealth::Unmeasured => {
            vec!["Refresh real measurement artifacts before claiming promise health.".to_string()]
        }
        PromiseHealth::Fail => vec![
            "Investigate over-budget paths or reached ceilings before presenting this promise as healthy."
                .to_string(),
        ],
        PromiseHealth::AtRisk if gaps_empty => {
            vec!["Extend the sweep to the promised scale before marking this promise healthy."
                .to_string()]
        }
        PromiseHealth::AtRisk => {
            vec!["Fill missing evidence before marking this promise healthy.".to_string()]
        }
        PromiseHealth::Pass => {
            vec!["Promise health is currently passing; keep the evidence fresh.".to_string()]
        }
    }
}
