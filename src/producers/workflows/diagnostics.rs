//! Render attribution uses per-trial intervals, not differences of aggregate medians.
use serde_json::{json, Value};

pub(super) fn annotate(payload: &mut Value) {
    let Some(rows) = payload["scenarios"].as_array_mut() else {
        return;
    };
    for row in rows
        .iter_mut()
        .filter(|row| row["latency_kind"] == "first_render")
    {
        let samples = row["raw_samples"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let interval = |sample: &Value| {
            let end = sample["first_render"].as_f64()?;
            let start = sample["first_result"].as_f64()?;
            (end >= start && start >= 0.0).then_some((end - start) / 1e6)
        };
        let summary = json!({
            "scope": "headless_first_result_delivery_to_tessellation",
            "delivery_to_render_ms": summarize(samples.iter().map(interval), samples.len()),
            "layout_ms": summarize(samples.iter().map(|s| s["first_render_layout"]["layout_time_ns"].as_f64().map(|v| v / 1e6)), samples.len()),
            "layout_input_bytes": summarize(samples.iter().map(|s| s["first_render_layout"]["layout_input_bytes"].as_f64()), samples.len()),
            "result_groups_rendered": summarize(samples.iter().map(|s| s["first_render_layout"]["result_groups_rendered"].as_f64()), samples.len()),
            "result_groups_skipped": summarize(samples.iter().map(|s| s["first_render_layout"]["result_groups_skipped"].as_f64()), samples.len()),
        });
        row["render_diagnostics"] = summary;
    }
}

fn summarize(values: impl Iterator<Item = Option<f64>>, expected: usize) -> Value {
    let mut values: Vec<_> = values
        .flatten()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    values.sort_by(f64::total_cmp);
    let count = values.len();
    let complete = count > 0 && count == expected;
    json!({"sample_count":count, "expected_count":expected, "complete":complete,
        "median":complete.then(|| (values[count / 2] + values[(count - 1) / 2]) / 2.0),
        "min":values.first(), "max":values.last()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attribution_uses_paired_trials_and_preserves_missing_counters() {
        let mut payload = json!({"scenarios":[{"latency_kind":"first_render","raw_samples":[
            {"first_result":1000000,"first_render":11000000,"first_render_layout":{"layout_time_ns":9000000,"result_groups_rendered":5,"result_groups_skipped":9995}},
            {"first_result":20000000,"first_render":22000000,"first_render_layout":{"layout_time_ns":1000000,"result_groups_rendered":6}}
        ]}]});
        annotate(&mut payload);
        let d = &payload["scenarios"][0]["render_diagnostics"];
        assert_eq!(d["delivery_to_render_ms"]["median"], 6.0);
        assert_eq!(d["layout_ms"]["median"], 5.0);
        assert_eq!(d["result_groups_rendered"]["median"], 5.5);
        assert_eq!(d["result_groups_skipped"]["complete"], false);
        assert!(d["result_groups_skipped"]["median"].is_null());
        payload["scenarios"][0]["raw_samples"][0]["first_result"] = json!(12000000);
        annotate(&mut payload);
        assert!(
            payload["scenarios"][0]["render_diagnostics"]["delivery_to_render_ms"]["median"]
                .is_null()
        );
    }
}
