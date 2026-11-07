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
            "non_layout_render_ms": summarize(samples.iter().map(|sample| {
                let elapsed = interval(sample)?;
                let layout = sample["first_render_layout"]["layout_time_ns"].as_f64()? / 1e6;
                (layout >= 0.0 && layout <= elapsed).then_some(elapsed - layout)
            }), samples.len()),
            "layout_jobs": summarize(samples.iter().map(|s| s["first_render_layout"]["layout_jobs"].as_f64()), samples.len()),
            "wrap_width_change_px": summarize(samples.iter().map(|s| {
                let layout = &s["first_render_layout"];
                if layout["word_wrap"] != true { return None; }
                Some((layout["result_wrap_width"].as_f64()? - layout["prepared_wrap_width"].as_f64()?).abs())
            }), samples.len()),
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
    use super::annotate;
    use serde_json::json;
    #[test]
    fn paired_render_attribution_preserves_missing_inconsistent_and_unknown_counters() {
        let mut payload = json!({"scenarios":[{"latency_kind":"first_render","raw_samples":[
            {"first_result":1000000,"first_render":11000000,"first_render_layout":{"layout_time_ns":9000000,"result_groups_rendered":5,"result_groups_skipped":9995,"word_wrap":true,"prepared_wrap_width":1263,"result_wrap_width":1256}},
            {"first_result":20000000,"first_render":22000000,"first_render_layout":{"layout_time_ns":1000000,"result_groups_rendered":6,"word_wrap":true,"prepared_wrap_width":1256,"result_wrap_width":1256}}
        ]}]});
        annotate(&mut payload);
        let d = &payload["scenarios"][0]["render_diagnostics"];
        for (field, expected) in [
            ("delivery_to_render_ms", 6.0),
            ("layout_ms", 5.0),
            ("non_layout_render_ms", 1.0),
            ("result_groups_rendered", 5.5),
        ] {
            assert_eq!(d[field]["median"], expected, "{field}");
        }
        assert_eq!(d["result_groups_skipped"]["complete"], false);
        assert!(d["result_groups_skipped"]["median"].is_null());
        assert_eq!(d["wrap_width_change_px"]["max"], 7.0);
        assert_eq!(d["wrap_width_change_px"]["complete"], true);
        for (pointer, value, metrics) in [
            (
                "/first_result",
                json!(12000000),
                &["delivery_to_render_ms", "non_layout_render_ms"][..],
            ),
            (
                "/first_render_layout/layout_time_ns",
                json!(12000000),
                &["non_layout_render_ms"][..],
            ),
            (
                "/first_render_layout/word_wrap",
                json!(false),
                &["wrap_width_change_px"][..],
            ),
        ] {
            let sample = &mut payload["scenarios"][0]["raw_samples"][0];
            let previous = std::mem::replace(sample.pointer_mut(pointer).unwrap(), value);
            annotate(&mut payload);
            for metric in metrics {
                let d = &payload["scenarios"][0]["render_diagnostics"][metric];
                assert_eq!(d["complete"], false, "{metric}");
                assert!(d["median"].is_null(), "{metric}");
            }
            if pointer.ends_with("word_wrap") {
                assert!(
                    super::super::super::render::render_search_workflows(&payload)
                        .contains("max wrap change=unknown")
                );
            }
            *payload["scenarios"][0]["raw_samples"][0]
                .pointer_mut(pointer)
                .unwrap() = previous;
        }
    }
}
