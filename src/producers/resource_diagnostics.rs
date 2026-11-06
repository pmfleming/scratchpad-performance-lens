//! Diagnostic phase observations, grouped by workload and validated per trial.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Describe observed scaling without fitting a complexity class or changing budgets.
pub(super) fn scaling(events: &[Value]) -> Value {
    let mut groups: BTreeMap<(&str, &str), BTreeMap<u64, Vec<&Value>>> = BTreeMap::new();
    let mut ungrouped = 0;
    for event in events {
        match (
            event["workload_unit"].as_str(),
            event["measurement_scope"].as_str(),
            event["workload_value"].as_u64(),
        ) {
            (Some(unit), Some(scope), Some(size))
                if size > 0 && !unit.is_empty() && !scope.is_empty() =>
            {
                groups
                    .entry((unit, scope))
                    .or_default()
                    .entry(size)
                    .or_default()
                    .push(event);
            }
            _ => ungrouped += 1,
        }
    }
    let observations: Vec<_> = groups.into_iter().map(|((unit,scope), workloads)| {
        let points: Vec<_> = workloads.into_iter().map(|(size,trials)| {
            let mut times: Vec<_> = trials.iter().filter(|e| e["status"] == "ok")
                .filter_map(|e| e["elapsed_ns"].as_u64().filter(|n| *n > 0)).collect();
            times.sort_unstable();
            let complete = !times.is_empty() && times.len() == trials.len();
            let median = complete.then(|| (times[times.len()/2] as f64 + times[(times.len()-1)/2] as f64)/2.0);
            json!({"workload_value":size,"trial_count":trials.len(),"observed_count":times.len(),
                "complete_for_observed_trials":complete,"median_elapsed_ms":median.map(|n| n/1e6),
                "median_ns_per_unit":median.map(|n| n/size as f64)})
        }).collect();
        let adjacent: Vec<_> = points.windows(2).map(|pair| {
            let workload_ratio = pair[1]["workload_value"].as_f64().unwrap()/pair[0]["workload_value"].as_f64().unwrap();
            let elapsed_ratio = pair[0]["median_elapsed_ms"].as_f64().zip(pair[1]["median_elapsed_ms"].as_f64()).map(|(a,b)| b/a);
            json!({"from_workload":pair[0]["workload_value"],"to_workload":pair[1]["workload_value"],
                "workload_multiplier":workload_ratio,"elapsed_multiplier":elapsed_ratio,
                "per_unit_cost_multiplier":elapsed_ratio.map(|r| r/workload_ratio)})
        }).collect();
        json!({"workload_unit":unit,"measurement_scope":scope,"points":points,"adjacent_steps":adjacent})
    }).collect();
    json!({"groups":observations,"ungrouped_sample_count":ungrouped,
        "interpretation":"observed_workload_medians_only_no_extrapolation_or_budget_override"})
}

pub(super) fn phases(events: &[Value]) -> Value {
    let mut workloads: BTreeMap<i64, Vec<&Value>> = BTreeMap::new();
    for event in events {
        if let Some(size) = event["workload_value"].as_i64() {
            workloads.entry(size).or_default().push(event);
        }
    }
    json!(workloads
        .into_iter()
        .map(|(size, trials)| {
            let keys: BTreeSet<_> = trials
                .iter()
                .filter_map(|e| e["phase_elapsed_ns"].as_object())
                .flat_map(|phases| phases.keys())
                .filter(|key| key.ends_with("_ns"))
                .collect();
            let phases: BTreeMap<_, _> = keys
                .into_iter()
                .map(|key| {
                    let mut values: Vec<_> = trials
                        .iter()
                        .filter(|e| e["status"] == "ok")
                        .filter_map(|e| {
                            let phase = e["phase_elapsed_ns"][key].as_u64()?;
                            (phase <= e["elapsed_ns"].as_u64()?).then_some(phase as f64 / 1e6)
                        })
                        .collect();
                    values.sort_by(f64::total_cmp);
                    let count = values.len();
                    let complete = count > 0 && count == trials.len();
                    (
                        key,
                        json!({"observed_count":count,"trial_count":trials.len(),
                "complete_for_observed_trials":complete,
                "median_ms":complete.then(|| (values[count / 2] + values[(count - 1) / 2]) / 2.0),
                "observed_min_ms":values.first(),"observed_max_ms":values.last()}),
                    )
                })
                .collect();
            json!({"workload_value":size,"phases":phases,
            "scope":"named_operation_intervals_may_overlap_not_a_partition",
            "statistical_claim":"observed_trials_only_no_population_tail_guarantee"})
        })
        .collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scaling_keeps_scopes_separate_and_does_not_hide_failed_trials() {
        let row = |size, ns, scope| json!({"workload_value":size,"elapsed_ns":ns,"workload_unit":"tabs","measurement_scope":scope,"status":"ok"});
        let mut events = vec![
            row(100, 1000000, "prepared_operation"),
            row(1000, 100000000, "prepared_operation"),
            row(1000, 1, "scenario_including_fixture_work"),
        ];
        let result = scaling(&events);
        assert_eq!(result["groups"].as_array().unwrap().len(), 2);
        assert_eq!(
            result["groups"][0]["adjacent_steps"][0]["workload_multiplier"],
            10.0
        );
        assert_eq!(
            result["groups"][0]["adjacent_steps"][0]["per_unit_cost_multiplier"],
            10.0
        );
        let mut failed = events[1].clone();
        failed["status"] = json!("panic");
        events.push(failed);
        let result = scaling(&events);
        assert!(result["groups"][0]["adjacent_steps"][0]["elapsed_multiplier"].is_null());
        assert_eq!(
            result["groups"][0]["points"][1]["complete_for_observed_trials"],
            false
        );
        events.push(json!({"workload_value":0,"elapsed_ns":1}));
        assert_eq!(scaling(&events)["ungrouped_sample_count"], 1);
    }
    #[test]
    fn keeps_workloads_units_and_incomplete_phases_separate() {
        let events = vec![
            json!({"status":"ok","workload_value":100,"elapsed_ns":10000000,"phase_elapsed_ns":{"build_ns":2000000,"input_bytes":500}}),
            json!({"status":"ok","workload_value":100,"elapsed_ns":10000000,"phase_elapsed_ns":{"build_ns":8000000}}),
            json!({"status":"ok","workload_value":200,"elapsed_ns":10000000,"phase_elapsed_ns":{"build_ns":12000000}}),
            json!({"status":"panic","workload_value":200,"elapsed_ns":10000000,"phase_elapsed_ns":{"build_ns":2000000}}),
        ];
        let result = phases(&events);
        assert_eq!(result[0]["phases"]["build_ns"]["median_ms"], 5.0);
        assert!(result[0]["phases"]["input_bytes"].is_null());
        assert!(result[1]["phases"]["build_ns"]["median_ms"].is_null());
        assert_eq!(
            result[1]["phases"]["build_ns"]["complete_for_observed_trials"],
            false
        );
    }
}
