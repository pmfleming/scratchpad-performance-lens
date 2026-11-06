use super::common::{
    annotate_measurement, benchmark_parameter, collect_benchmarks, dispersion_lower_ns,
    dispersion_upper_ns, ns_per_kb, parameter_label, run_benchmarks, score_of, search_signals,
    stability_label, throughput_mb_s, validate_measurements, variance_ratio,
};
use super::render::render_search;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use serde_json::{json, Value};
pub fn search_speed(config: &LensConfig, options: MeasureOptions) -> Result<()> {
    let probe_failure = run_benchmarks(
        &config.project_root,
        options.skip_bench,
        &["cargo", "bench", "--bench", "search_speed"],
        "Running focused search benchmarks via cargo bench...",
    );
    if let Some(error) = &probe_failure {
        eprintln!("Search benchmarking failed: {error}");
        eprintln!("Falling back to existing Criterion search results.");
    }

    let (mut rows, invalid) = collect_benchmarks(
        &config.project_root,
        &["search_speed"],
        |meta| {
            meta["workload_family"]
                .as_str()
                .is_some_and(|family| family.starts_with("search"))
        },
        build_search_row,
    );
    rows.sort_by(|left, right| {
        score_of(right).total_cmp(&score_of(left)).then_with(|| {
            left["name"]
                .as_str()
                .unwrap_or("")
                .cmp(right["name"].as_str().unwrap_or(""))
        })
    });
    let payload = Value::Array(rows);
    let cli = render_search(&payload);
    shared::write_visibility(
        &config.output_dir.join("search_speed.json"),
        &payload,
        "search-speed",
        cli,
    )?;
    validate_measurements(
        &payload,
        &invalid,
        probe_failure,
        options.fail_on_slow,
        "search",
    )
}

fn build_search_row(name: &str, key: &str, meta: &Value, estimates: &Value) -> Value {
    let parameter_value = benchmark_parameter(name);
    let parameter_unit = shared::string_field(meta, "parameter_unit", "value");
    let (item_count, bytes_per_item, total_bytes) =
        search_workload(meta, parameter_value, &parameter_unit);
    let mean_ns = shared::f64_field(estimates, &["mean", "point_estimate"]);
    let std_dev_ns = shared::f64_field(estimates, &["std_dev", "point_estimate"]);
    let dispersion_upper = dispersion_upper_ns(mean_ns, std_dev_ns);
    let median_ns = shared::f64_field(estimates, &["median", "point_estimate"]);
    let processed_bytes = meta["processed_item_count"]
        .as_i64()
        .zip(bytes_per_item)
        .and_then(|(count, bytes)| count.checked_mul(bytes))
        .or(total_bytes);
    let throughput_mb_s = throughput_mb_s(processed_bytes, mean_ns);
    let ns_per_kb = ns_per_kb(processed_bytes, mean_ns);
    let threshold_ms = shared::authoritative_threshold_ms(meta);
    let latency_kind = shared::string_field(meta, "latency_kind", "completion");
    let matching = shared::matching_flamegraph_ids(key);
    let signals = search_signals(
        mean_ns,
        std_dev_ns,
        threshold_ms.unwrap_or(0.0),
        &latency_kind,
        !matching.is_empty(),
    );
    let mut row = json!({
        "name": name,
        "measurement_status": "valid",
        "measurement_scope": shared::string_field(meta, "measurement_scope", "unknown"),
        "benchmark_key": key,
        "scenario_label": search_scenario_label(key),
        "description": shared::string_field(meta, "description", key),
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
        "fixed_item_count": meta["fixed_item_count"].as_i64(),
        "item_count": item_count,
        "bytes_per_item": bytes_per_item,
        "total_bytes": total_bytes,
        "processed_bytes": processed_bytes,
        "throughput_unit": "MiB/s",
        "throughput_scope": "actual_scanned_bytes_only",
        "response_match_limit": meta["response_match_limit"].as_i64(),
        "mean_ns": mean_ns,
        "std_dev_ns": std_dev_ns,
        "median_ns": median_ns,
        "dispersion_upper_ns": dispersion_upper,
        "dispersion_lower_ns": dispersion_lower_ns(mean_ns, std_dev_ns),
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
    annotate_measurement(&mut row, estimates, meta);
    row
}

fn search_workload(
    meta: &Value,
    parameter_value: Option<i64>,
    parameter_unit: &str,
) -> (Option<i64>, Option<i64>, Option<i64>) {
    let fixed_count = meta["fixed_item_count"].as_i64();
    if parameter_unit == "bytes" {
        (
            fixed_count.or(Some(1)),
            parameter_value,
            parameter_value.and_then(|value| value.checked_mul(fixed_count.unwrap_or(1))),
        )
    } else {
        let bytes_per_item = meta["bytes_per_item"].as_i64();
        (
            parameter_value,
            bytes_per_item,
            parameter_value
                .zip(bytes_per_item)
                .and_then(|(items, bytes)| items.checked_mul(bytes)),
        )
    }
}

fn search_scenario_label(key: &str) -> String {
    key.trim_start_matches("search_")
        .replace('_', " ")
        .split_whitespace()
        .map(shared::capitalize)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn first_target_throughput_does_not_claim_unscanned_targets() {
        let row = super::build_search_row(
            "first/32",
            "first",
            &serde_json::json!({"parameter_unit":"files","bytes_per_item":16384,"processed_item_count":1}),
            &serde_json::json!({"mean":{"point_estimate":1_000_000}}),
        );
        assert_eq!(row["total_bytes"], 32 * 16384);
        assert_eq!(row["processed_bytes"], 16384);
        assert_eq!(row["throughput_mb_s"], 15.625);
        for unit in ["bytes", "files"] {
            let meta = serde_json::json!({"fixed_item_count":2,"bytes_per_item":2});
            assert_eq!(super::search_workload(&meta, Some(i64::MAX), unit).2, None);
            assert_eq!(super::search_workload(&meta, Some(32), unit).2, Some(64));
            assert_eq!(super::search_workload(&meta, None, unit).2, None);
        }
    }
}
