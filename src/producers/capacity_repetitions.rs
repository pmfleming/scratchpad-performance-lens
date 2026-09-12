use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub(super) fn aggregate(events: Vec<Value>) -> Vec<Value> {
    let mut by_step: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
    for event in events {
        by_step.entry(event["step_index"].as_i64().unwrap_or(-1)).or_default().push(event);
    }
    by_step.into_values().map(aggregate_step).collect()
}

fn aggregate_step(repetitions: Vec<Value>) -> Value {
    let first = &repetitions[0];
    let expected = first["expected_repetition_count"].as_u64().unwrap_or(3);
    let mut identities = HashSet::new();
    let valid_identity = repetitions.iter().all(|event| {
        event["workload_value"] == first["workload_value"]
            && event["workload_unit"] == first["workload_unit"]
            && event["measurement_scope"] == first["measurement_scope"]
            && event["expected_repetition_count"].as_u64().unwrap_or(3) == expected
            && event["repeat_index"].as_u64().is_some_and(|index| index < expected && identities.insert(index))
            && event["elapsed_ns"].as_u64().is_some_and(|ns| ns > 0)
    });
    let complete = expected >= 3 && repetitions.len() as u64 == expected && valid_identity;
    let failure_count = repetitions.iter().filter(|event| event["status"] != "ok").count();
    // Timing describes successful trials only; failed trials remain in raw_repetitions.
    let mut successful: Vec<_> = repetitions.iter().filter(|event| event["status"] == "ok"
        && event["elapsed_ns"].as_u64().is_some_and(|ns| ns > 0)).collect();
    successful.sort_by_key(|event| event["elapsed_ns"].as_u64());
    let mut result = successful.get(successful.len()/2).copied().unwrap_or(first).clone();
    let successful_events: Vec<_> = successful.iter().map(|event| (*event).clone()).collect();
    for key in ["elapsed_ns", "setup_elapsed_ns", "background_completion_ns"] {
        result[key] = median(&successful_events, key);
    }
    result["elapsed_min_ns"] = successful.first().map_or(Value::Null, |event| event["elapsed_ns"].clone());
    result["elapsed_max_ns"] = successful.last().map_or(Value::Null, |event| event["elapsed_ns"].clone());
    for key in ["peak_working_set_bytes", "working_set_bytes", "handle_count", "page_fault_count"] {
        result[key] = json!(repetitions.iter().filter_map(|event| event[key].as_u64()).max());
    }
    result["status"] = json!(if failure_count > 0 { "failed" } else if !complete { "incomplete" } else { "ok" });
    result["complete"] = json!(complete);
    result["failure_count"] = json!(failure_count);
    result["repetition_count"] = json!(repetitions.len());
    result["successful_repetition_count"] = json!(successful.len());
    result["expected_repetition_count"] = json!(expected);
    result["statistical_claim"] = json!("successful-trial median; no population tail guarantee");
    result["raw_repetitions"] = json!(repetitions);
    result
}

fn median(events: &[Value], key: &str) -> Value {
    let mut values: Vec<_> = events.iter().filter_map(|event| event[key].as_u64()).collect();
    values.sort_unstable();
    if values.is_empty() { return Value::Null; }
    let midpoint = values.len()/2;
    if values.len() % 2 == 0 {
        json!(values[midpoint-1] as f64 / 2.0 + values[midpoint] as f64 / 2.0)
    } else { json!(values[midpoint]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(index: usize, elapsed: u64, status: &str, peak: u64) -> Value {
        json!({"repeat_index":index,"step_index":0,"elapsed_ns":elapsed,"status":status,
            "workload_value":100,"workload_unit":"tabs","measurement_scope":"prepared_operation",
            "peak_working_set_bytes":peak})
    }
    #[test]
    fn panic_cannot_disappear_behind_successful_median() {
        let row = aggregate(vec![sample(0, 5,"panic",999),sample(1,10,"ok",10),sample(2,20,"ok",20)]).remove(0);
        assert_eq!(row["elapsed_ns"],15.0);
        assert_eq!(row["status"],"failed");
        assert_eq!(row["failure_count"],1);
        assert_eq!(row["peak_working_set_bytes"],999);
        assert_eq!(row["raw_repetitions"].as_array().unwrap().len(),3);
    }
    #[test]
    fn missing_duplicate_or_inconsistent_repetitions_are_incomplete() {
        for events in [vec![sample(0,10,"ok",10)],
            vec![sample(0,10,"ok",10),sample(0,20,"ok",20),sample(2,30,"ok",30)]] {
            let row = aggregate(events).remove(0);
            assert_eq!(row["complete"],false);
            assert_eq!(row["status"],"incomplete");
        }
        let mut wrong = sample(2,30,"ok",30); wrong["workload_value"] = json!(999);
        assert_eq!(aggregate(vec![sample(0,10,"ok",10),sample(1,20,"ok",20),wrong])[0]["complete"],false);
    }
}
