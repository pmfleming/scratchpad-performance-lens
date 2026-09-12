use super::common::{probe_path, run_probe_object, write_probe_artifact};
use crate::{cli::MeasureOptions, config::LensConfig};
use anyhow::Result;
use serde_json::json;

fn valid_trials(row: &serde_json::Value) -> bool {
    let (Some(samples), Some(count), Some(budget), Some(probe)) = (
        row["sample_elapsed_ms"].as_array(),
        row["repetition_count"].as_u64(),
        row["threshold_ms"].as_f64(),
        row["budget_probe_ns"].as_f64(),
    ) else {
        return false;
    };
    let Some(mut values) = samples
        .iter()
        .map(serde_json::Value::as_f64)
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    if count == 0 || count as usize != values.len() || values.iter().any(|ms| *ms <= 0.0) {
        return false;
    }
    values.sort_by(f64::total_cmp);
    let median = (values[values.len() / 2] + values[(values.len() - 1) / 2]) / 2.0;
    let status = if median > budget {
        "fail"
    } else if count >= 30 && values.last().is_some_and(|ms| *ms <= budget) {
        "pass"
    } else {
        "uncertain"
    };
    row["budget_status"] == status && (probe / 1e6 - median).abs() <= 1e-9 * median.max(1.0)
}

fn valid_workflows(payload: &serde_json::Value) -> bool {
    let Some(rows) = payload["scenarios"].as_array() else {
        return false;
    };
    rows.len() == 12
        && ["active", "current", "all"].iter().all(|mode| {
            ["dispatch", "first_result", "first_render", "completion"]
                .iter()
                .all(|operation| {
                    let key = format!("search_workflow_{mode}_{operation}");
                    rows.iter()
                        .filter(|row| row["benchmark_key"] == key)
                        .count()
                        == 1
                        && rows.iter().any(|row| {
                            row["benchmark_key"] == key
                                && valid_trials(row)
                                && row["measurement_status"] == "valid"
                                && row["threshold_authoritative"] == true
                                && row["threshold_ms"]
                                    .as_f64()
                                    .is_some_and(|value| value > 0.0)
                                && row["budget_probe_ns"]
                                    .as_f64()
                                    .is_some_and(|value| value > 0.0)
                                && row["measurement_scope"]
                                    == format!("query_command_to_{operation}")
                                && row["repetition_count"].as_u64().is_some_and(|count| {
                                    count > 0
                                        && row["sample_elapsed_ms"].as_array().is_some_and(
                                            |samples| {
                                                samples.len() as u64 == count
                                                    && samples.iter().all(|sample| {
                                                        sample
                                                            .as_f64()
                                                            .is_some_and(|value| value > 0.0)
                                                    })
                                            },
                                        )
                                })
                        })
                })
        })
}

pub fn search_workflows(config: &LensConfig, _: MeasureOptions) -> Result<()> {
    let result = run_probe_object(
        &config.project_root,
        &[
            "cargo",
            "build",
            "--release",
            "--quiet",
            "--bin",
            "search_workflow_metrics",
        ],
        probe_path("search_workflow_metrics"),
    )
    .and_then(|payload| {
        anyhow::ensure!(
            valid_workflows(&payload),
            "invalid or incomplete search workflow measurements"
        );
        Ok(payload)
    });
    let (payload, failure) = match result {
        Ok(payload) => (payload, None),
        Err(error) => (
            json!({"meta":{"probe_status":"failed","error":error.to_string()},"scenarios":[]}),
            Some(error.to_string()),
        ),
    };
    write_probe_artifact(
        &config.output_dir.join("search_workflow_metrics.json"),
        &payload,
        "search workflows",
        super::render::render_search_workflows(&payload),
        failure,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_exposes_workflow_failures_and_unknown_measurements() {
        let text = super::super::render::render_search_workflows(&json!({"scenarios":[{
            "name":"search_workflow_all_dispatch/10000", "median_ms":280.0,
            "max_ms":300.0, "threshold_ms":50.0, "budget_status":"fail"},
            {"name":"incomplete"}]}));
        assert!(text.contains("median=280.000 ms"));
        assert!(text.contains("status=fail"));
        assert!(text.contains("median=unknown"));
    }

    #[test]
    fn workflow_flags_must_agree_with_observations_and_repetition_count() {
        let mut row = json!({"sample_elapsed_ms":[1.0],"repetition_count":1,"threshold_ms":2,
            "budget_probe_ns":1_000_000,"budget_status":"pass"});
        assert!(!valid_trials(&row));
        row["budget_status"] = json!("uncertain");
        assert!(valid_trials(&row));
        row["sample_elapsed_ms"] = json!(vec![1.0; 30]);
        row["repetition_count"] = json!(30);
        row["budget_status"] = json!("pass");
        assert!(valid_trials(&row));
        row["budget_probe_ns"] = json!(999);
        assert!(!valid_trials(&row));
        assert!(!valid_workflows(&json!({"scenarios":[]})));
    }
}
