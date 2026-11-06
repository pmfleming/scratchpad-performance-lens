//! Lane counters cover the complete profile, including background completion.
//! They are busy wall time, can overlap, and are not CPU or critical-path time.
use serde_json::{json, Value};

pub(super) fn background_lanes(event: &Value) -> Value {
    let trials = event["raw_repetitions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let expected = event["expected_repetition_count"].as_u64();
    let mut lanes = serde_json::Map::new();
    for lane in ["path", "session", "analysis"] {
        let key = format!("background_io_{lane}_active_ns");
        let mut values: Vec<_> = trials
            .iter()
            .filter(|trial| trial["status"] == "ok")
            .filter_map(|trial| trial["metrics"][&key].as_u64())
            .map(|ns| ns as f64 / 1e6)
            .collect();
        values.sort_by(f64::total_cmp);
        let count = values.len();
        let complete = event["complete"] == true && count > 0 && expected == Some(count as u64);
        lanes.insert(
            lane.into(),
            json!({
                "sample_count": count, "expected_count": expected, "complete": complete,
                "median_ms": complete.then(|| (values[count / 2] + values[(count - 1) / 2]) / 2.0),
                "observed_min_ms": values.first(), "observed_max_ms": values.last()
            }),
        );
    }
    json!({
        "scope": "complete_profile_through_background_completion",
        "quantity": "overlapping_lane_busy_wall_time_not_cpu_or_critical_path",
        "lanes": lanes
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_raw_trials_instead_of_the_representative_event() {
        let mut event = json!({"complete":true,"expected_repetition_count":3,
        "metrics":{"background_io_path_active_ns":999000000},
        "raw_repetitions":[
            {"status":"ok","metrics":{"background_io_path_active_ns":1000000}},
            {"status":"ok","metrics":{"background_io_path_active_ns":7000000}},
            {"status":"ok","metrics":{"background_io_path_active_ns":3000000}}
        ]});
        let result = background_lanes(&event);
        assert_eq!(result["lanes"]["path"]["median_ms"], 3.0);
        assert_eq!(result["lanes"]["session"]["complete"], false);
        assert!(result["lanes"]["session"]["median_ms"].is_null());
        event["raw_repetitions"][1]["status"] = json!("panic");
        let result = background_lanes(&event);
        assert_eq!(result["lanes"]["path"]["sample_count"], 2);
        assert_eq!(result["lanes"]["path"]["complete"], false);
        assert!(result["lanes"]["path"]["median_ms"].is_null());
    }
}
