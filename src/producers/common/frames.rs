//! Shared frame evidence contracts, independent of any report's presentation.
use serde_json::Value;

pub(in crate::producers) fn valid_frame(row: &Value) -> bool {
    ["p95_ms", "p99_ms", "budget_ms", "p99_budget_ms"]
        .iter()
        .all(|key| {
            row[*key]
                .as_f64()
                .is_some_and(|value| value.is_finite() && value > 0.0)
        })
        && row["p95_ms"].as_f64() <= row["p99_ms"].as_f64()
        && row["max_ms"].as_f64().is_none_or(|maximum| {
            row["p99_ms"].as_f64().is_some_and(|p99| {
                p99 <= maximum + row["percentile_resolution_ms"].as_f64().unwrap_or(0.0)
            })
        })
        && row["frame_count"].as_u64().is_some_and(|count| count > 0)
        && row["measurement_scope"]
            .as_str()
            .is_some_and(|scope| !scope.is_empty() && scope != "unknown")
}

pub(in crate::producers) fn frame_over_budget(row: &Value) -> bool {
    valid_frame(row)
        && (row["p95_ms"].as_f64() > row["budget_ms"].as_f64()
            || row["p99_ms"].as_f64() > row["p99_budget_ms"].as_f64())
}
