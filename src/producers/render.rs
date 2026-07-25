use super::common::{scenarios_array, values_array};
use crate::shared;
use serde_json::Value;

fn text<'a>(value: &'a Value, key: &str, default: &'a str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn integer(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn array_field<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn render_lines(header: &str, rows: Vec<String>, empty: Option<&str>) -> String {
    let mut lines = vec![header.to_string()];
    if rows.is_empty() {
        lines.extend(empty.map(str::to_string));
    } else {
        lines.extend(rows);
    }
    lines.join("\n")
}

pub(super) fn render_slowspots(payload: &Value) -> String {
    let rows = values_array(payload)
        .iter()
        .take(10)
        .enumerate()
        .map(|(index, item)| {
            format!(
                "{:>2}. {} | family={} | mean={:.2}ms | score={:.2} | {}",
                index + 1,
                text(item, "name", "-"),
                text(item, "workload_family", "unmapped"),
                shared::f64_field(item, &["mean_ns"]) / 1_000_000.0,
                shared::f64_field(item, &["score"]),
                text(item, "signals", "nominal"),
            )
        })
        .collect();
    render_lines("Slowspots", rows, Some("No slowspots found."))
}

pub(super) fn render_search(payload: &Value) -> String {
    let rows = values_array(payload)
        .iter()
        .take(10)
        .enumerate()
        .map(|(index, item)| {
            let throughput = item
                .get("throughput_mb_s")
                .and_then(Value::as_f64)
                .map_or("-".to_string(), |value| format!("{value:.1} MB/s"));
            format!(
                "{:>2}. {}/{}/{} | param={} | mean={:.2}ms | throughput={} | {}",
                index + 1,
                text(item, "mode", "-"),
                text(item, "latency_kind", "-"),
                text(item, "scaling_axis", "-"),
                text(item, "parameter_label", "-"),
                shared::f64_field(item, &["mean_ns"]) / 1_000_000.0,
                throughput,
                text(item, "signals", ""),
            )
        })
        .collect();
    render_lines(
        "Search Speed",
        rows,
        Some("No search-speed benchmarks found."),
    )
}

pub(super) fn render_frame_metrics(payload: &Value) -> String {
    let rows = scenarios_array(payload)
        .iter()
        .map(|scenario| {
            let fps = scenario
                .get("theoretical_fps_p99")
                .and_then(Value::as_f64)
                .map_or(String::new(), |fps| {
                    format!(" | theoretical_p99_fps={fps:.0}")
                });
            format!(
                "- {}: scope={} | p95={:.2} ms, p99={:.2} ms, budget={:.2} ms{}",
                text(scenario, "scenario_label", "-"),
                text(scenario, "measurement_scope", "measured_frame_path_cpu"),
                shared::f64_field(scenario, &["p95_ms"]),
                shared::f64_field(scenario, &["p99_ms"]),
                shared::f64_field(scenario, &["budget_ms"]),
                fps,
            )
        })
        .collect();
    render_lines(
        "Frame Metrics",
        rows,
        Some("No frame metrics were produced."),
    )
}

pub(super) fn render_capacity(payload: &Value) -> String {
    let rows = scenarios_array(payload)
        .iter()
        .map(|item| {
            let ceiling = item
                .get("first_failure_label")
                .or_else(|| item.get("last_successful_label"))
                .and_then(Value::as_str)
                .unwrap_or("-");
            format!(
                "- {}: ceiling={} | mode={} | resource={}",
                text(item, "scenario_label", "-"),
                ceiling,
                text(item, "failure_mode", "-"),
                text(item, "suspected_limiting_resource", "-"),
            )
        })
        .collect();
    render_lines(
        "Capacity Report",
        rows,
        Some("No capacity scenarios recorded."),
    )
}

pub(super) fn render_resources(payload: &Value) -> String {
    let rows = scenarios_array(payload)
        .iter()
        .map(|item| {
            let mut row = format!(
                "- {}: max_elapsed={:.1} ms | peak_heap={} | total_alloc={} | max_ws={}",
                text(item, "scenario_label", "-"),
                shared::f64_field(item, &["max_elapsed_ms"]),
                shared::human_bytes(item.get("max_peak_live_bytes").and_then(Value::as_i64)),
                shared::human_bytes(item.get("max_allocated_bytes").and_then(Value::as_i64)),
                shared::human_bytes(item.get("max_working_set_bytes").and_then(Value::as_i64)),
            );
            if let Some(setup_ms) = item.get("max_setup_elapsed_ms").and_then(Value::as_f64) {
                if setup_ms > 0.0 {
                    row.push_str(&format!(" | setup={setup_ms:.1} ms"));
                }
            }
            if let (Some(retained), Some(limit)) = (
                item.get("max_retained_file_chunks").and_then(Value::as_i64),
                item.get("file_chunk_cache_limit").and_then(Value::as_i64),
            ) {
                row.push_str(&format!(
                    " | retained_chunks={retained}/{limit} | cache_bound_held={}",
                    item.get("cache_bound_held")
                        .and_then(Value::as_bool)
                        .map_or("-", |held| if held { "yes" } else { "no" })
                ));
            }
            row
        })
        .collect();
    render_lines(
        "Resource Profiles",
        rows,
        Some("No resource profiles recorded."),
    )
}

pub(super) fn render_flamegraphs(payload: &Value) -> String {
    let rows = values_array(payload)
        .iter()
        .map(|item| {
            if item
                .get("available")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                format!(
                    "  [x] {}: {}",
                    text(item, "name", "-"),
                    text(item, "path", "-")
                )
            } else {
                format!(
                    "  [ ] {} | {}",
                    text(item, "name", "-"),
                    text(item, "issue", "not generated")
                )
            }
        })
        .collect();
    render_lines("Flamegraph Results:", rows, None)
}

pub(super) fn render_speed_report(payload: &Value) -> String {
    let rows = array_field(payload, "triage")
        .iter()
        .enumerate()
        .map(|(index, item)| {
            format!(
                "{:>2}. {} | family={} | resource={} | {}",
                index + 1,
                text(item, "scenario_label", "-"),
                text(item, "family", "-"),
                text(item, "suspected_limiting_resource", "-"),
                text(item, "recommended_action", "-"),
            )
        })
        .collect();
    render_lines(
        "Speed And Efficiency Report",
        rows,
        Some("No investigation candidates were found."),
    )
}

pub(super) fn render_performance_review(payload: &Value) -> String {
    let rows = scenarios_array(payload)
        .iter()
        .map(|scenario| {
            format!(
                "- {}: health={} | coverage={} | score={:.2} | gaps={}",
                text(scenario, "title", "-"),
                text(scenario, "promise_health", "-"),
                text(scenario, "coverage_status", "-"),
                shared::f64_field(scenario, &["coverage_score"]),
                array_field(scenario, "gaps").len(),
            )
        })
        .collect();
    render_lines("Performance Review Promise Health", rows, None)
}

pub(super) fn render_project_code(payload: &Value) -> String {
    let current = payload.get("current").unwrap_or(&Value::Null);
    let latest = payload.get("latest_push").unwrap_or(&Value::Null);
    [
        "Project Code Metrics".to_string(),
        format!("- Ref: {}", text(payload, "ref", "-")),
        format!(
            "- Latest GitHub push: {} {}",
            text(latest, "short_sha", "-"),
            text(latest, "date", "-")
        ),
        format!("- Application code: {}", integer(current, "application")),
        format!("- Test code: {}", integer(current, "test")),
        format!("- Other code: {}", integer(current, "other")),
        format!("- Total code: {}", integer(current, "total")),
    ]
    .join("\n")
}
