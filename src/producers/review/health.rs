use super::registry::ReviewScenario;
use crate::artifacts::capacity_report::CapacityScenario;
use crate::artifacts::performance_review::{
    CeilingStatus, CoverageAxes, CoverageAxis, CoverageStatus, PromiseHealth, ScaleCheck,
    ScaleCheckStatus,
};
use serde_json::Value;
pub(super) fn coverage_axis(
    expected: &[&str],
    available: Vec<String>,
    valid: Vec<String>,
) -> CoverageAxis {
    let missing_evidence: Vec<_> = expected
        .iter()
        .filter(|id| !valid.iter().any(|value| value == **id))
        .map(|id| id.to_string())
        .collect();
    CoverageAxis {
        required: !expected.is_empty(),
        covered: if expected.is_empty() {
            !valid.is_empty()
        } else {
            missing_evidence.is_empty()
        },
        count: available.len(),
        available_count: available.len(),
        valid_count: valid.len(),
        expected_count: expected.len(),
        missing_evidence,
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
    .map(|(name, axis)| {
        format!(
            "Missing valid {name} evidence: {}.",
            axis.missing_evidence.join(", ")
        )
    })
    .collect()
}

pub(super) fn scale_checks(
    scenario: &ReviewScenario,
    capacity_rows: &[CapacityScenario],
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
                .filter(|row| target.capacity_scenarios.contains(&row.scenario.as_str()))
                .collect();
            let matching_resources: Vec<_> = resource_rows
                .iter()
                .filter(|row| row_scenario_matches(row, target.resource_scenarios))
                .collect();
            let max_workload_value = matching_capacity
                .iter()
                .filter_map(|row| row.max_workload_value())
                .chain(
                    matching_resources
                        .iter()
                        .filter_map(|row| max_resource_workload_value(row)),
                )
                .max();
            let first_failure_workload = matching_capacity
                .iter()
                .filter_map(|row| row.first_failure_workload)
                .min();
            // Every required operation must demonstrate the target independently.
            // Resource workload sizes cannot substitute for capacity latency evidence.
            let demonstrated: Vec<Option<i64>> = if !target.capacity_scenarios.is_empty() {
                target
                    .capacity_scenarios
                    .iter()
                    .map(|id| {
                        matching_capacity
                            .iter()
                            .find(|row| row.scenario == *id)
                            .and_then(|row| row.demonstrated_workload())
                    })
                    .collect()
            } else {
                target
                    .resource_scenarios
                    .iter()
                    .map(|id| {
                        matching_resources
                            .iter()
                            .find(|row| row["scenario"].as_str() == Some(*id))
                            .and_then(|row| demonstrated_resource_workload(row))
                    })
                    .collect()
            };
            let last_successful_workload = demonstrated
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>()
                .and_then(|values| values.into_iter().min());
            let synthetic = (capacity_synthetic && !matching_capacity.is_empty())
                || (resources_synthetic && !matching_resources.is_empty());
            let failed_capacity_scenarios = matching_capacity
                .iter()
                .filter(|row| {
                    ceiling_failed_before_promise(
                        target.minimum,
                        row.first_failure_workload,
                        row.last_successful_workload,
                    )
                })
                .map(|row| row.scenario.clone())
                .collect::<Vec<_>>();
            let failed_before_promise = !failed_capacity_scenarios.is_empty();
            let met = !synthetic
                && !failed_before_promise
                && !demonstrated.is_empty()
                && last_successful_workload.is_some_and(|value| value >= target.minimum);
            let ceiling_status =
                ceiling_status(synthetic, failed_before_promise, first_failure_workload);
            ScaleCheck {
                id: target.id.to_string(),
                label: target.label.to_string(),
                minimum: target.minimum,
                unit: target.unit.to_string(),
                max_workload_value,
                first_failure_workload,
                last_successful_workload,
                headroom_workload: headroom_workload(target.minimum, last_successful_workload),
                evidence_count: matching_capacity.len() + matching_resources.len(),
                failed_capacity_scenarios,
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
    failed_before_promise: bool,
    first_failure_workload: Option<i64>,
) -> CeilingStatus {
    if synthetic {
        CeilingStatus::Unmeasured
    } else if failed_before_promise {
        CeilingStatus::FailedBeforePromise
    } else if first_failure_workload.is_some() {
        CeilingStatus::FailedAfterPromise
    } else {
        CeilingStatus::NotReached
    }
}

fn headroom_workload(minimum: i64, last_successful_workload: Option<i64>) -> Option<i64> {
    last_successful_workload
        .filter(|success| *success >= minimum)
        .map(|value| value - minimum)
}

fn row_scenario_matches(row: &Value, scenario_ids: &[&str]) -> bool {
    row.get("scenario")
        .and_then(Value::as_str)
        .is_some_and(|id| scenario_ids.contains(&id))
}

fn demonstrated_resource_workload(row: &Value) -> Option<i64> {
    let threshold = row
        .get("threshold_ms")?
        .as_f64()
        .filter(|value| *value > 0.0)?;
    row.get("samples")?
        .as_array()?
        .iter()
        .filter(|sample| {
            sample["status"].as_str() == Some("ok")
                && sample["elapsed_ms"]
                    .as_f64()
                    .is_some_and(|value| value > 0.0 && value <= threshold)
        })
        .filter_map(|sample| sample["workload_value"].as_i64())
        .max()
}

fn max_resource_workload_value(row: &Value) -> Option<i64> {
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
    if budget_misses > 0 || scale_failures > 0 {
        // Known real failures must not be hidden by unrelated missing evidence.
        PromiseHealth::Fail
    } else if synthetic_evidence || coverage_status == CoverageStatus::Missing {
        PromiseHealth::Unmeasured
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
