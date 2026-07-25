use super::common::{
    approx_p05_ns, approx_p95_ns, criterion_dir, estimate_optional, latency_score, over_threshold,
    score_of, slowspot_signals, stability_label,
};
use super::render::render_slowspots;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{bail, Result};
use serde_json::{json, Value};
pub fn slowspots(config: &LensConfig, options: MeasureOptions) -> Result<()> {
    let mut probe_failure = (!config.project_root.is_dir()).then(|| {
        format!(
            "project root does not exist: {}",
            config.project_root.display()
        )
    });
    if !options.skip_bench && probe_failure.is_none() {
        if let Err(error) = shared::run_progress_command(
            &config.project_root,
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
        ) {
            eprintln!("Benchmarking failed: {error}");
            probe_failure = Some(error.to_string());
        }
    }

    let metadata = shared::load_benchmark_metadata(&config.project_root);
    let criterion_dir = criterion_dir(&config.project_root);
    let mut rows = Vec::new();
    for path in shared::criterion_estimate_paths(&criterion_dir) {
        let Some(name) = shared::benchmark_name_from_estimate_path(&criterion_dir, &path) else {
            continue;
        };
        let key = shared::benchmark_key(&name).to_string();
        let Some(meta) = metadata.get(&key) else {
            eprintln!("Skipping stale or unmapped Criterion result: {name}");
            continue;
        };
        let estimates = shared::read_json(&path).into_value_or(json!({}));
        let mean_ns = shared::f64_field(&estimates, &["mean", "point_estimate"]);
        let std_dev_ns = shared::f64_field(&estimates, &["std_dev", "point_estimate"]);
        let approx_p95_ns = approx_p95_ns(mean_ns, std_dev_ns);
        let median_ns = shared::f64_field(&estimates, &["median", "point_estimate"]);
        let dispersion_ns = estimate_optional(&estimates, &["median_abs_dev", "point_estimate"]);
        let threshold_ms = shared::authoritative_threshold_ms(meta);
        let threshold_signal_ms = threshold_ms.unwrap_or(0.0);
        let matching = shared::matching_flamegraph_ids(&key);
        let stability = stability_label(mean_ns, std_dev_ns);
        let score = latency_score(mean_ns, std_dev_ns);
        let targets = shared::array_strings(meta, "targets");
        let signals = slowspot_signals(
            mean_ns,
            std_dev_ns,
            threshold_signal_ms,
            targets.is_empty(),
            !matching.is_empty(),
        );
        let mut row = json!({
            "name": name,
            "mean_ns": mean_ns,
            "std_dev_ns": std_dev_ns,
            "median_ns": median_ns,
            "approx_p95_ns": approx_p95_ns,
            "approx_p05_ns": approx_p05_ns(mean_ns, std_dev_ns),
            "budget_probe_ns": approx_p95_ns,
            "budget_probe_label": "approx_p95_from_mean_std_dev",
            "dispersion_ns": dispersion_ns,
            "dispersion_label": "median_abs_dev",
            "score": score,
            "signals": signals,
            "benchmark_key": key,
            "targets": targets,
            "benchmark_kind": shared::string_field(meta, "kind", "unmapped"),
            "workload_family": shared::string_field(meta, "workload_family", "unmapped"),
            "threshold_ms": threshold_ms,
            "matching_flamegraphs": matching,
            "has_profile_coverage": !matching.is_empty(),
            "stability": stability,
            "suspected_limiting_resource": shared::string_field(meta, "limiting_resource_hint", "cpu"),
            "probe_class": "targeted_path",
            "measurement_role": "change_validation",
            "measurement_question": "Did this implementation path stay inside its latency budget?",
        });
        row["threshold_authoritative"] = json!(threshold_ms.is_some());
        row["threshold_source"] = meta.get("threshold_source").cloned().unwrap_or(Value::Null);
        row["metadata_source"] = meta.get("metadata_source").cloned().unwrap_or(Value::Null);
        row["stale_budget_risk"] = json!(meta
            .get("stale_budget_risk")
            .and_then(Value::as_bool)
            .unwrap_or(false));
        rows.push(row);
    }
    rows.sort_by(|left, right| score_of(right).total_cmp(&score_of(left)));
    let payload = Value::Array(rows);
    let cli = render_slowspots(&payload);
    shared::write_visibility(
        &config.output_dir.join("slowspots.json"),
        &payload,
        "slowspot",
        cli,
    )?;
    if let Some(error) = probe_failure {
        bail!("slowspots probe failed: {error}");
    }
    if options.fail_on_slow
        && payload
            .as_array()
            .is_some_and(|rows| rows.iter().any(over_threshold))
    {
        bail!("one or more benchmarks exceeded threshold");
    }
    Ok(())
}
