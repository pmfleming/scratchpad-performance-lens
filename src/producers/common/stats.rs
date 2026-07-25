use crate::shared;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(in crate::producers) fn run_benchmarks(
    project_root: &Path,
    skip: bool,
    cargo_args: &[&str],
    label: &str,
) -> Option<String> {
    if !project_root.is_dir() {
        let root = project_root.display();
        return Some(format!("project root does not exist: {root}"));
    }
    if skip {
        return None;
    }
    shared::run_progress_command(project_root, cargo_args, label)
        .err()
        .map(|error| error.to_string())
}

pub(in crate::producers) fn criterion_dir(project_root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| project_root.join("target"))
        .join("criterion")
}

pub(in crate::producers) fn estimate_optional(value: &Value, path: &[&str]) -> Option<f64> {
    let result = shared::f64_field(value, path);
    (result != 0.0).then_some(result)
}

pub(in crate::producers) fn variance_ratio(mean_ns: f64, std_dev_ns: f64) -> f64 {
    if mean_ns > 0.0 {
        std_dev_ns / mean_ns
    } else {
        0.0
    }
}

pub(in crate::producers) fn stability_label(mean_ns: f64, std_dev_ns: f64) -> &'static str {
    let ratio = variance_ratio(mean_ns, std_dev_ns);
    if mean_ns <= 0.0 {
        "unknown"
    } else if ratio >= 0.2 {
        "high-variance"
    } else if ratio >= 0.1 {
        "watch"
    } else {
        "stable"
    }
}

pub(in crate::producers) fn latency_score(mean_ns: f64, std_dev_ns: f64) -> f64 {
    let score = mean_ns / 1_000_000.0 * (1.0 + variance_ratio(mean_ns, std_dev_ns));
    (score * 100.0).round() / 100.0
}

pub(in crate::producers) fn approx_p95_ns(mean_ns: f64, std_dev_ns: f64) -> f64 {
    (mean_ns + (2.0 * std_dev_ns)).max(0.0)
}

pub(in crate::producers) fn approx_p05_ns(mean_ns: f64, std_dev_ns: f64) -> f64 {
    (mean_ns - (2.0 * std_dev_ns)).max(0.0)
}

pub(in crate::producers) fn slowspot_signals(
    mean_ns: f64,
    std_dev_ns: f64,
    threshold_ms: f64,
    unmapped: bool,
    has_profiles: bool,
) -> String {
    let mut signals = Vec::new();
    let budget_probe_ms = approx_p95_ns(mean_ns, std_dev_ns) / 1_000_000.0;
    if threshold_ms > 0.0 && budget_probe_ms > threshold_ms {
        signals.push(format!("slow > {threshold_ms}ms"));
    }
    if mean_ns > 1_000_000.0 && variance_ratio(mean_ns, std_dev_ns) > 0.2 {
        signals.push("high variance".to_string());
    }
    if unmapped {
        signals.push("unmapped benchmark".to_string());
    }
    if has_profiles {
        signals.push("profile coverage".to_string());
    }
    if signals.is_empty() {
        "nominal".to_string()
    } else {
        signals.join(", ")
    }
}

pub(in crate::producers) fn search_signals(
    mean_ns: f64,
    std_dev_ns: f64,
    threshold_ms: f64,
    latency_kind: &str,
    has_profiles: bool,
) -> String {
    let mut signals = Vec::new();
    let budget_probe_ms = approx_p95_ns(mean_ns, std_dev_ns) / 1_000_000.0;
    if threshold_ms > 0.0 && budget_probe_ms > threshold_ms {
        signals.push(format!("over budget > {threshold_ms:.1}ms"));
    }
    signals.push(
        if latency_kind == "first_response" {
            "partial-result latency"
        } else {
            "full-scan latency"
        }
        .to_string(),
    );
    if mean_ns > 0.0 && variance_ratio(mean_ns, std_dev_ns) > 0.2 {
        signals.push("high variance".to_string());
    }
    if has_profiles {
        signals.push("profile coverage".to_string());
    }
    signals.join(", ")
}

pub(in crate::producers) fn benchmark_parameter(name: &str) -> Option<i64> {
    name.split_once('/')?.1.parse().ok()
}

pub(in crate::producers) fn parameter_label(value: Option<i64>, unit: &str) -> String {
    match value {
        Some(value) if unit == "bytes" => shared::human_bytes(Some(value)),
        Some(value) => format!("{value} {unit}"),
        None => "-".to_string(),
    }
}

pub(in crate::producers) fn throughput_mb_s(total_bytes: Option<i64>, mean_ns: f64) -> Option<f64> {
    let total_bytes = total_bytes?;
    if total_bytes <= 0 || mean_ns <= 0.0 {
        return None;
    }
    Some(total_bytes as f64 * 1_000_000_000.0 / mean_ns / (1024.0 * 1024.0))
}

pub(in crate::producers) fn ns_per_kb(total_bytes: Option<i64>, mean_ns: f64) -> Option<f64> {
    let total_bytes = total_bytes?;
    if total_bytes <= 0 || mean_ns <= 0.0 {
        return None;
    }
    Some(mean_ns / (total_bytes as f64 / 1024.0))
}

pub(in crate::producers) fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

pub(in crate::producers) fn over_threshold(row: &Value) -> bool {
    let budget_probe_ms = budget_probe_ms(row).unwrap_or(0.0);
    let threshold_ms = row
        .get("threshold_ms")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    threshold_ms > 0.0 && budget_probe_ms > threshold_ms
}

pub(in crate::producers) fn budget_probe_ms(row: &Value) -> Option<f64> {
    row.get("p99_ms")
        .or_else(|| row.get("p95_ms"))
        .and_then(Value::as_f64)
        .or_else(|| {
            row.get("budget_probe_ns")
                .or_else(|| row.get("approx_p95_ns"))
                .and_then(Value::as_f64)
                .map(|value| value / 1_000_000.0)
        })
        .or_else(|| {
            row.get("mean_ms").and_then(Value::as_f64).or_else(|| {
                row.get("mean_ns")
                    .and_then(Value::as_f64)
                    .map(|value| value / 1_000_000.0)
            })
        })
}

pub(in crate::producers) fn score_of(row: &Value) -> f64 {
    row.get("score").and_then(Value::as_f64).unwrap_or(0.0)
}

pub(in crate::producers) fn string_value(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        budget_probe_ms, latency_score, ns_per_kb, over_threshold, stability_label, throughput_mb_s,
    };

    #[test]
    fn stability_labels_follow_variance_thresholds() {
        assert_eq!(stability_label(100.0, 5.0), "stable");
        assert_eq!(stability_label(100.0, 15.0), "watch");
        assert_eq!(stability_label(100.0, 25.0), "high-variance");
        assert_eq!(stability_label(0.0, 25.0), "unknown");
    }

    #[test]
    fn latency_score_increases_with_variance() {
        assert!(latency_score(1_000_000.0, 500_000.0) > latency_score(1_000_000.0, 0.0));
        assert_eq!(latency_score(0.0, 50.0), 0.0);
    }

    #[test]
    fn budget_checks_prefer_tail_probe_over_mean() {
        let row = serde_json::json!({
            "mean_ns": 80_000_000.0,
            "approx_p95_ns": 130_000_000.0,
            "threshold_ms": 100.0,
        });
        assert!(over_threshold(&row));
        assert_eq!(budget_probe_ms(&row), Some(130.0));
    }

    #[test]
    fn throughput_and_cost_guard_zero_inputs() {
        assert_eq!(
            throughput_mb_s(Some(1024 * 1024), 1_000_000_000.0),
            Some(1.0)
        );
        assert_eq!(throughput_mb_s(Some(0), 1_000_000_000.0), None);
        assert_eq!(ns_per_kb(Some(1024), 2048.0), Some(2048.0));
        assert_eq!(ns_per_kb(Some(0), 2048.0), None);
    }
}
