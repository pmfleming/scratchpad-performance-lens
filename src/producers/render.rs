use super::common::{scenarios_array, values_array};
use crate::shared;
use serde_json::Value;

fn text<'a>(value: &'a Value, key: &str, default: &'a str) -> &'a str {
    value[key].as_str().unwrap_or(default)
}

fn integer(value: &Value, key: &str) -> i64 {
    value[key].as_i64().unwrap_or(0)
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

fn measured_ms(value: &Value, key: &str, scale: f64) -> String {
    value[key]
        .as_f64()
        .map_or_else(|| "unknown".into(), |value| format!("{:.3}", value / scale))
}

pub(super) fn render_search_workflows(payload: &Value) -> String {
    let rows = scenarios_array(payload)
        .iter()
        .map(|row| {
            let mut line = format!(
                "- {}: median={} ms | max={} ms | budget={} ms | status={}",
                text(row, "name", "unknown"),
                measured_ms(row, "median_ms", 1.0),
                measured_ms(row, "max_ms", 1.0),
                measured_ms(row, "threshold_ms", 1.0),
                text(row, "budget_status", "unmeasured"),
            );
            if row["render_diagnostics"].is_object() {
                let d = &row["render_diagnostics"];
                let width = &d["wrap_width_change_px"];
                let max_wrap_change = if width["complete"] == true {
                    measured_ms(width, "max", 1.0)
                } else {
                    "unknown".to_owned()
                };
                line.push_str(&format!(
                    " | delivery-to-render={} ms | layout={} ms | other render={} ms | max wrap change={} px | groups rendered={} skipped={}",
                    measured_ms(&d["delivery_to_render_ms"], "median", 1.0),
                    measured_ms(&d["layout_ms"], "median", 1.0),
                    measured_ms(&d["non_layout_render_ms"], "median", 1.0),
                    max_wrap_change,
                    measured_ms(&d["result_groups_rendered"], "median", 1.0),
                    measured_ms(&d["result_groups_skipped"], "median", 1.0)
                ));
            }
            line
        })
        .collect();
    render_lines(
        "Production Search Workflows (headless; no display present)",
        rows,
        Some("No valid search workflows were produced."),
    )
}

pub(super) fn render_slowspots(payload: &Value) -> String {
    let rows = values_array(payload)
        .iter()
        .take(10)
        .enumerate()
        .map(|(index, item)| {
            format!(
                "{:>2}. {} | family={} | mean={}ms | score={:.2} | budget={} | {}",
                index + 1,
                text(item, "name", "-"),
                text(item, "workload_family", "unmapped"),
                measured_ms(item, "mean_ns", 1_000_000.0),
                shared::f64_field(item, &["score"]),
                text(item, "budget_status", "unmeasured"),
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
            let throughput = item["throughput_mb_s"]
                .as_f64()
                .map_or("-".to_string(), |value| format!("{value:.1} MB/s"));
            format!(
                "{:>2}. {}/{}/{} | param={} | mean={}ms | throughput={} | budget={} | {}",
                index + 1,
                text(item, "mode", "-"),
                text(item, "latency_kind", "-"),
                text(item, "scaling_axis", "-"),
                text(item, "parameter_label", "-"),
                measured_ms(item, "mean_ns", 1_000_000.0),
                throughput,
                text(item, "budget_status", "unmeasured"),
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
                "- {}: max_elapsed={} | peak_heap={} | total_alloc={} | max_ws={}",
                text(item, "scenario_label", "-"),
                item["max_elapsed_ms"]
                    .as_f64()
                    .map_or_else(|| "unknown".into(), |ms| format!("{ms:.1} ms")),
                shared::human_bytes(item["max_peak_live_bytes"].as_i64()),
                shared::human_bytes(item["max_allocated_bytes"].as_i64()),
                shared::human_bytes(item["max_working_set_bytes"].as_i64()),
            );
            if let Some(save_ms) = item["max_save_ms"].as_f64() {
                row.push_str(&format!(
                    " | max_save={save_ms:.1} ms | max_write_bytes/save={}",
                    shared::human_bytes(item["max_write_bytes_per_operation"].as_i64())
                ));
            }
            if let Some(ratio) = item["max_write_amplification"].as_f64() {
                row.push_str(&format!(" | max_process_write/text={ratio:.2}x"));
            }
            if let Some(setup_ms) = item["max_setup_elapsed_ms"].as_f64() {
                if setup_ms > 0.0 {
                    row.push_str(&format!(" | setup={setup_ms:.1} ms"));
                }
            }
            if let (Some(retained), Some(limit)) = (
                item["max_retained_file_chunks"].as_i64(),
                item["file_chunk_cache_limit"].as_i64(),
            ) {
                row.push_str(&format!(
                    " | retained_chunks={retained}/{limit} | cache_bound_held={}",
                    item["cache_bound_held"]
                        .as_bool()
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
            if item["available"].as_bool().unwrap_or(false) {
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
    let rows = values_array(&payload["triage"])
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
                values_array(&scenario["gaps"]).len(),
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
