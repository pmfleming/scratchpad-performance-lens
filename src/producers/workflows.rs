use super::common::{run_probe_object, write_probe_artifact};
use crate::{cli::MeasureOptions, config::LensConfig};
use anyhow::Result;
use serde_json::json;

mod diagnostics;

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
    let result = (|| -> Result<serde_json::Value> {
        let run = || run_probe_object(&config.project_root, "search_workflow_metrics");
        let mut payload = run()?;
        anyhow::ensure!(
            valid_workflows(&payload),
            "invalid or incomplete search workflow measurements"
        );
        let near = payload["scenarios"].as_array().unwrap().iter().any(|row| {
            row["median_ms"]
                .as_f64()
                .zip(row["threshold_ms"].as_f64())
                .is_some_and(|(ms, budget)| budget > 0.0 && (0.9..=1.1).contains(&(ms / budget)))
        });
        if near {
            for _ in 0..2 {
                let more = run()?;
                anyhow::ensure!(valid_workflows(&more), "invalid search workflow repeat");
                merge_trials(&mut payload, &more)?;
            }
        }
        diagnostics::annotate(&mut payload);
        Ok(payload)
    })();
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

fn merge_trials(payload: &mut serde_json::Value, more: &serde_json::Value) -> Result<()> {
    for row in payload["scenarios"].as_array_mut().unwrap() {
        let extra = more["scenarios"]
            .as_array()
            .unwrap()
            .iter()
            .find(|extra| extra["name"] == row["name"])
            .ok_or_else(|| anyhow::anyhow!("workflow repeat changed inventory"))?;
        anyhow::ensure!(
            row["threshold_ms"] == extra["threshold_ms"]
                && row["measurement_scope"] == extra["measurement_scope"],
            "workflow repeat changed contract"
        );
        for key in ["sample_elapsed_ms", "raw_samples"] {
            if let Some(values) = extra[key].as_array() {
                row[key]
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("missing workflow samples"))?
                    .extend(values.iter().cloned());
            }
        }
        let mut values: Vec<_> = row["sample_elapsed_ms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        values.sort_by(f64::total_cmp);
        let count = values.len();
        let median = (values[count / 2] + values[(count - 1) / 2]) / 2.0;
        let maximum = values[count - 1];
        let budget = row["threshold_ms"].as_f64().unwrap();
        row["repetition_count"] = json!(count);
        row["median_ms"] = json!(median);
        row["max_ms"] = json!(maximum);
        row["mean_ms"] = json!(values.iter().sum::<f64>() / count as f64);
        row["budget_probe_ns"] = json!(median * 1e6);
        row["budget_status"] = json!(if median > budget {
            "fail"
        } else if count >= 30 && maximum <= budget {
            "pass"
        } else {
            "uncertain"
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{valid_trials, valid_workflows};
    use serde_json::json;
    #[test]
    fn borderline_repeats_keep_all_trials_and_recompute_verdicts() {
        let run = |ms| {
            json!({"scenarios":[{"name":"search/1","threshold_ms":2,
            "measurement_scope":"query_command_to_completion","sample_elapsed_ms":vec![ms;30],
            "raw_samples":vec![json!({"completion":ms * 1e6});30]}]})
        };
        let mut combined = run(1.0);
        super::merge_trials(&mut combined, &run(3.0)).unwrap();
        assert_eq!(combined["scenarios"][0]["budget_status"], "uncertain");
        super::merge_trials(&mut combined, &run(3.0)).unwrap();
        assert_eq!(combined["scenarios"][0]["repetition_count"], 90);
        assert_eq!(
            combined["scenarios"][0]["raw_samples"]
                .as_array()
                .unwrap()
                .len(),
            90
        );
        assert_eq!(combined["scenarios"][0]["budget_status"], "fail");
        assert!(valid_trials(&combined["scenarios"][0]));
    }
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
