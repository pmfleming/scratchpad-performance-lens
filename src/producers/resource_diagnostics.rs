//! Diagnostic phase observations, grouped by workload and validated per trial.
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

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
