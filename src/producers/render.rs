use super::common::{array_value, scenarios_array};
use crate::shared;
use serde_json::Value;
pub(super) fn render_slowspots(payload: &Value) -> String {
    let mut lines = vec!["Slowspots".to_string()];
    for (index, item) in array_value(payload).iter().take(10).enumerate() {
        lines.push(format!(
            "{:>2}. {} | family={} | mean={:.2}ms | score={:.2} | {}",
            index + 1,
            item.get("name").and_then(Value::as_str).unwrap_or("-"),
            item.get("workload_family")
                .and_then(Value::as_str)
                .unwrap_or("unmapped"),
            item.get("mean_ns").and_then(Value::as_f64).unwrap_or(0.0) / 1_000_000.0,
            item.get("score").and_then(Value::as_f64).unwrap_or(0.0),
            item.get("signals")
                .and_then(Value::as_str)
                .unwrap_or("nominal"),
        ));
    }
    if array_value(payload).is_empty() {
        lines.push("No slowspots found.".to_string());
    }
    lines.join("\n")
}

pub(super) fn render_search(payload: &Value) -> String {
    let mut lines = vec!["Search Speed".to_string()];
    for (index, item) in array_value(payload).iter().take(10).enumerate() {
        let throughput = item
            .get("throughput_mb_s")
            .and_then(Value::as_f64)
            .map_or("-".to_string(), |value| format!("{value:.1} MB/s"));
        lines.push(format!(
            "{:>2}. {}/{}/{} | param={} | mean={:.2}ms | throughput={} | {}",
            index + 1,
            item.get("mode").and_then(Value::as_str).unwrap_or("-"),
            item.get("latency_kind")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("scaling_axis")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("parameter_label")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("mean_ns").and_then(Value::as_f64).unwrap_or(0.0) / 1_000_000.0,
            throughput,
            item.get("signals").and_then(Value::as_str).unwrap_or(""),
        ));
    }
    if array_value(payload).is_empty() {
        lines.push("No search-speed benchmarks found.".to_string());
    }
    lines.join("\n")
}

pub(super) fn render_frame_metrics(payload: &Value) -> String {
    let mut lines = vec!["Frame Metrics".to_string()];
    for scenario in scenarios_array(payload) {
        lines.push(format!(
            "- {}: scope={} | p95={:.2} ms, p99={:.2} ms, budget={:.2} ms{}",
            scenario
                .get("scenario_label")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            scenario
                .get("measurement_scope")
                .and_then(Value::as_str)
                .unwrap_or("measured_frame_path_cpu"),
            scenario
                .get("p95_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            scenario
                .get("p99_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            scenario
                .get("budget_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            scenario
                .get("theoretical_fps_p99")
                .and_then(Value::as_f64)
                .map_or(String::new(), |fps| format!(
                    " | theoretical_p99_fps={fps:.0}"
                )),
        ));
    }
    if scenarios_array(payload).is_empty() {
        lines.push("No frame metrics were produced.".to_string());
    }
    lines.join("\n")
}

pub(super) fn render_capacity(payload: &Value) -> String {
    let mut lines = vec!["Capacity Report".to_string()];
    for item in scenarios_array(payload) {
        lines.push(format!(
            "- {}: ceiling={} | mode={} | resource={}",
            item.get("scenario_label")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("first_failure_label")
                .or_else(|| item.get("last_successful_label"))
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("failure_mode")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("suspected_limiting_resource")
                .and_then(Value::as_str)
                .unwrap_or("-"),
        ));
    }
    if scenarios_array(payload).is_empty() {
        lines.push("No capacity scenarios recorded.".to_string());
    }
    lines.join("\n")
}

pub(super) fn render_resources(payload: &Value) -> String {
    let mut lines = vec!["Resource Profiles".to_string()];
    for item in scenarios_array(payload) {
        lines.push(format!(
            "- {}: max_elapsed={:.1} ms | max_alloc={} | max_ws={}",
            item.get("scenario_label")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("max_elapsed_ms")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            shared::human_bytes(item.get("max_allocated_bytes").and_then(Value::as_i64)),
            shared::human_bytes(item.get("max_working_set_bytes").and_then(Value::as_i64)),
        ));
    }
    if scenarios_array(payload).is_empty() {
        lines.push("No resource profiles recorded.".to_string());
    }
    lines.join("\n")
}

pub(super) fn render_flamegraphs(payload: &Value) -> String {
    let mut lines = vec!["Flamegraph Results:".to_string()];
    for item in array_value(payload) {
        if item
            .get("available")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            lines.push(format!(
                "  [x] {}: {}",
                item.get("name").and_then(Value::as_str).unwrap_or("-"),
                item.get("path").and_then(Value::as_str).unwrap_or("-")
            ));
        } else {
            lines.push(format!(
                "  [ ] {} | {}",
                item.get("name").and_then(Value::as_str).unwrap_or("-"),
                item.get("issue")
                    .and_then(Value::as_str)
                    .unwrap_or("not generated")
            ));
        }
    }
    lines.join("\n")
}

pub(super) fn render_speed_report(payload: &Value) -> String {
    let mut lines = vec!["Speed And Efficiency Report".to_string()];
    let triage = payload
        .get("triage")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, item) in triage.iter().enumerate() {
        lines.push(format!(
            "{:>2}. {} | family={} | resource={} | {}",
            index + 1,
            item.get("scenario_label")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("family").and_then(Value::as_str).unwrap_or("-"),
            item.get("suspected_limiting_resource")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            item.get("recommended_action")
                .and_then(Value::as_str)
                .unwrap_or("-"),
        ));
    }
    if triage.is_empty() {
        lines.push("No investigation candidates were found.".to_string());
    }
    lines.join("\n")
}

pub(super) fn render_performance_review(payload: &Value) -> String {
    let mut lines = vec!["Performance Review Promise Health".to_string()];
    for scenario in payload
        .get("scenarios")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        lines.push(format!(
            "- {}: health={} | coverage={} | score={:.2} | gaps={}",
            scenario.get("title").and_then(Value::as_str).unwrap_or("-"),
            scenario
                .get("promise_health")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            scenario
                .get("coverage_status")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            scenario
                .get("coverage_score")
                .and_then(Value::as_f64)
                .unwrap_or(0.0),
            scenario
                .get("gaps")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        ));
    }
    lines.join("\n")
}

pub(super) fn render_project_code(payload: &Value) -> String {
    let current = payload.get("current").unwrap_or(&Value::Null);
    let latest = payload.get("latest_push").unwrap_or(&Value::Null);
    [
        "Project Code Metrics".to_string(),
        format!(
            "- Ref: {}",
            payload.get("ref").and_then(Value::as_str).unwrap_or("-")
        ),
        format!(
            "- Latest GitHub push: {} {}",
            latest
                .get("short_sha")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            latest.get("date").and_then(Value::as_str).unwrap_or("-")
        ),
        format!(
            "- Application code: {}",
            current
                .get("application")
                .and_then(Value::as_i64)
                .unwrap_or(0)
        ),
        format!(
            "- Test code: {}",
            current.get("test").and_then(Value::as_i64).unwrap_or(0)
        ),
        format!(
            "- Other code: {}",
            current.get("other").and_then(Value::as_i64).unwrap_or(0)
        ),
        format!(
            "- Total code: {}",
            current.get("total").and_then(Value::as_i64).unwrap_or(0)
        ),
    ]
    .join("\n")
}
