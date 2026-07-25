use super::common::{
    approx_p05_ns, approx_p95_ns, benchmark_parameter, capitalize, criterion_dir, ns_per_kb,
    over_threshold, parameter_label, score_of, search_signals, stability_label, string_value,
    throughput_mb_s, variance_ratio,
};
use super::render::render_search;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{bail, Result};
use serde_json::{json, Value};
pub fn search_speed(config: &LensConfig, options: MeasureOptions) -> Result<()> {
    let mut probe_failure = (!config.project_root.is_dir()).then(|| {
        format!(
            "project root does not exist: {}",
            config.project_root.display()
        )
    });
    if !options.skip_bench && probe_failure.is_none() {
        if let Err(error) = shared::run_progress_command(
            &config.project_root,
            &["cargo", "bench", "--bench", "search_speed"],
            "Running focused search benchmarks via cargo bench...",
        ) {
            eprintln!("Search benchmarking failed: {error}");
            eprintln!("Falling back to existing Criterion search results.");
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
            continue;
        };
        if !shared::string_field(meta, "workload_family", "").starts_with("search") {
            continue;
        }
        let estimates = shared::read_json(&path).into_value_or(json!({}));
        let parameter_value = benchmark_parameter(&name);
        let parameter_unit = shared::string_field(meta, "parameter_unit", "value");
        let item_count = if parameter_unit == "bytes" {
            meta.get("fixed_item_count")
                .and_then(Value::as_i64)
                .or(Some(1))
        } else {
            parameter_value
        };
        let bytes_per_item = if parameter_unit == "bytes" {
            parameter_value
        } else {
            meta.get("bytes_per_item").and_then(Value::as_i64)
        };
        let total_bytes = if parameter_unit == "bytes" {
            parameter_value.map(|value| {
                value
                    * meta
                        .get("fixed_item_count")
                        .and_then(Value::as_i64)
                        .unwrap_or(1)
            })
        } else {
            parameter_value
                .zip(bytes_per_item)
                .map(|(items, bytes)| items * bytes)
        };
        let mean_ns = shared::f64_field(&estimates, &["mean", "point_estimate"]);
        let std_dev_ns = shared::f64_field(&estimates, &["std_dev", "point_estimate"]);
        let approx_p95_ns = approx_p95_ns(mean_ns, std_dev_ns);
        let median_ns = shared::f64_field(&estimates, &["median", "point_estimate"]);
        let throughput_mb_s = throughput_mb_s(total_bytes, mean_ns);
        let ns_per_kb = ns_per_kb(total_bytes, mean_ns);
        let threshold_ms = shared::authoritative_threshold_ms(meta);
        let threshold_signal_ms = threshold_ms.unwrap_or(0.0);
        let latency_kind = shared::string_field(meta, "latency_kind", "completion");
        let matching = shared::matching_flamegraph_ids(&key);
        let signals = search_signals(
            mean_ns,
            std_dev_ns,
            threshold_signal_ms,
            &latency_kind,
            !matching.is_empty(),
        );
        let mut row = json!({
            "name": name,
            "benchmark_key": key,
            "scenario_label": key.trim_start_matches("search_").replace('_', " ").split_whitespace().map(capitalize).collect::<Vec<_>>().join(" "),
            "description": shared::string_field(meta, "description", &key),
            "mode": shared::string_field(meta, "mode", "unknown"),
            "latency_kind": latency_kind,
            "scaling_axis": shared::string_field(meta, "scaling_axis", "aggregate_size"),
            "benchmark_kind": shared::string_field(meta, "kind", "workflow"),
            "workload_family": shared::string_field(meta, "workload_family", "search"),
            "parameter_value": parameter_value,
            "parameter_unit": parameter_unit,
            "parameter_label": parameter_label(parameter_value, &parameter_unit),
            "query": shared::string_field(meta, "query", ""),
            "targets": shared::array_strings(meta, "targets"),
            "threshold_ms": threshold_ms,
            "fixed_item_count": meta.get("fixed_item_count").and_then(Value::as_i64),
            "item_count": item_count,
            "bytes_per_item": bytes_per_item,
            "total_bytes": total_bytes,
            "response_match_limit": meta.get("response_match_limit").and_then(Value::as_i64),
            "mean_ns": mean_ns,
            "std_dev_ns": std_dev_ns,
            "median_ns": median_ns,
            "approx_p95_ns": approx_p95_ns,
            "approx_p05_ns": approx_p05_ns(mean_ns, std_dev_ns),
            "budget_probe_ns": approx_p95_ns,
            "budget_probe_label": "approx_p95_from_mean_std_dev",
            "throughput_mb_s": throughput_mb_s,
            "ns_per_kb": ns_per_kb,
            "score": ns_per_kb.map_or((mean_ns / 1_000_000.0 * 100.0).round() / 100.0, |value| (value * (1.0 + variance_ratio(mean_ns, std_dev_ns)) * 100.0).round() / 100.0),
            "signals": signals,
            "matching_flamegraphs": matching,
            "has_profile_coverage": !matching.is_empty(),
            "stability": stability_label(mean_ns, std_dev_ns),
            "suspected_limiting_resource": shared::string_field(meta, "limiting_resource_hint", "cpu"),
            "probe_class": "targeted_path",
            "measurement_role": "change_validation",
            "measurement_question": "Did this search path stay inside its latency budget?",
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
    rows.sort_by(|left, right| {
        score_of(right)
            .total_cmp(&score_of(left))
            .then_with(|| string_value(left, "name").cmp(&string_value(right, "name")))
    });
    let payload = Value::Array(rows);
    let cli = render_search(&payload);
    shared::write_visibility(
        &config.output_dir.join("search_speed.json"),
        &payload,
        "search-speed",
        cli,
    )?;
    if let Some(error) = probe_failure {
        bail!("search probe failed: {error}");
    }
    if options.fail_on_slow
        && payload
            .as_array()
            .is_some_and(|rows| rows.iter().any(over_threshold))
    {
        bail!("one or more search benchmarks exceeded threshold");
    }
    Ok(())
}
