use super::common::{
    annotate_measurement, collect_benchmarks, dispersion_lower_ns, dispersion_upper_ns,
    estimate_optional, latency_score, run_benchmarks, score_of, slowspot_signals, stability_label,
    validate_measurements,
};
use super::render::render_slowspots;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};

pub fn slowspots(config: &LensConfig, options: MeasureOptions) -> Result<()> {
    let probe_failure = run_benchmarks(
        &config.project_root,
        options.skip_bench,
        &[
            "cargo",
            "bench",
            "--bench",
            "search_speed",
            "--bench",
            "frame_budget",
            "--bench",
            "promise_latency",
        ],
        "Running benchmarks via cargo bench...",
    );
    if let Some(error) = &probe_failure {
        eprintln!("Benchmarking failed: {error}");
    }
    let (mut rows, invalid) = collect_benchmarks(
        &config.project_root,
        &["search_speed", "frame_budget", "promise_latency"],
        |_| true,
        build_slowspot_row,
    );
    rows.sort_by(|left, right| score_of(right).total_cmp(&score_of(left)));
    let payload = Value::Array(rows);
    shared::write_visibility(
        &config.output_dir.join("slowspots.json"),
        &payload,
        "slowspot",
        render_slowspots(&payload),
    )?;
    validate_measurements(
        &payload,
        &invalid,
        probe_failure,
        options.fail_on_slow,
        "slowspots",
    )
}

fn build_slowspot_row(name: &str, key: &str, meta: &Value, estimates: &Value) -> Value {
    let mean_ns = shared::f64_field(estimates, &["mean", "point_estimate"]);
    let std_dev_ns = shared::f64_field(estimates, &["std_dev", "point_estimate"]);
    let threshold_ms = shared::authoritative_threshold_ms(meta);
    let matching = shared::matching_flamegraph_ids(key);
    let targets = shared::array_strings(meta, "targets");
    let signals = slowspot_signals(
        mean_ns,
        std_dev_ns,
        threshold_ms.unwrap_or(0.0),
        targets.is_empty(),
        !matching.is_empty(),
    );
    let mut row = json!({
        "name": name,
        "measurement_status": "valid",
        "measurement_scope": shared::string_field(meta, "measurement_scope", "prepared_component"),
        "mean_ns": mean_ns,
        "std_dev_ns": std_dev_ns,
        "median_ns": shared::f64_field(estimates, &["median", "point_estimate"]),
        "dispersion_upper_ns": dispersion_upper_ns(mean_ns, std_dev_ns),
        "dispersion_lower_ns": dispersion_lower_ns(mean_ns, std_dev_ns),
        "dispersion_ns": estimate_optional(estimates, &["median_abs_dev", "point_estimate"]),
        "dispersion_label": "median_abs_dev",
        "score": latency_score(mean_ns, std_dev_ns),
        "signals": signals,
        "benchmark_key": key,
        "targets": targets,
        "benchmark_kind": shared::string_field(meta, "kind", "unmapped"),
        "workload_family": shared::string_field(meta, "workload_family", "unmapped"),
        "threshold_ms": threshold_ms,
        "matching_flamegraphs": matching,
        "has_profile_coverage": !matching.is_empty(),
        "stability": stability_label(mean_ns, std_dev_ns),
        "suspected_limiting_resource": shared::string_field(meta, "limiting_resource_hint", "cpu"),
        "probe_class": "targeted_path",
        "measurement_role": "change_validation",
        "measurement_question": "Did this implementation path stay inside its latency budget?",
    });
    annotate_measurement(&mut row, estimates, meta);
    row
}
