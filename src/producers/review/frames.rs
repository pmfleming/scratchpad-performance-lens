use crate::shared;
use serde_json::{json, Value};

pub(super) fn frame_rows(payload: &Value) -> Vec<Value> {
    payload["scenarios"].as_array().into_iter().flatten().cloned().map(|mut row| {
        let id = row["scenario_id"].as_str().unwrap_or("").to_owned();
        let key = shared::benchmark_key(&id);
        row["name"] = json!(id);
        row["benchmark_key"] = json!(key);
        row["evidence_kind"] = json!("frame");
        row["threshold_ms"] = row["budget_ms"].clone();
        row["threshold_authoritative"] = json!(true);
        row["measurement_status"] = json!(if valid_frame(&row) { "valid" } else { "invalid" });
        // Explicit mapping: wheel-scroll is not the same family as component scroll benchmarks.
        if matches!(key, "ui_render_frame_120hz" | "editor_scroll_frame_120hz" | "editor_scroll_event_to_tessellation") {
            row["promise_scenarios"] = json!(["large_files"]);
        }
        row
    }).collect()
}

pub(in crate::producers) fn valid_frame(row: &Value) -> bool {
    ["p95_ms", "p99_ms", "budget_ms", "p99_budget_ms"].iter()
        .all(|key| row[*key].as_f64().is_some_and(|value| value.is_finite() && value > 0.0))
        && row["frame_count"].as_u64().is_some_and(|count| count > 0)
        && row["measurement_scope"].as_str().is_some_and(|scope| !scope.is_empty() && scope != "unknown")
}

pub(super) fn over_budget(row: &Value) -> bool {
    valid_frame(row) && (row["p95_ms"].as_f64() > row["budget_ms"].as_f64()
        || row["p99_ms"].as_f64() > row["p99_budget_ms"].as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::producers::review::{build_review_scenario, review_scenarios};
    use crate::artifacts::performance_review::PromiseHealth;
    #[test]
    fn either_frame_tail_budget_changes_promise_health_and_keeps_scope() {
        let scenario = review_scenarios().into_iter().find(|s| s.id == "large_files").unwrap();
        for (p95, p99, missed) in [(7.0, 10.0, false), (9.0, 10.0, true), (7.0, 20.0, true)] {
            let payload = json!({"scenarios":[{"scenario_id":"editor_scroll_event_to_tessellation/4194304",
                "measurement_scope":"end_to_end_event_to_tessellation", "present_included":false,
                "p95_ms":p95,"p99_ms":p99,"budget_ms":8.33,"p99_budget_ms":12,"frame_count":240}]});
            let row = build_review_scenario(&scenario, &frame_rows(&payload), &[], &[], vec![], false, false);
            assert_eq!(row.promise_health == PromiseHealth::Fail, missed);
            assert_eq!(row.evidence.latency[0]["present_included"], false);
        }
    }
}
