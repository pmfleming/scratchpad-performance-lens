use super::common::{probe_path, run_probe_object};
use super::render::render_frame_metrics;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{bail, Result};
use serde_json::{json, Value};

const REALISTIC_FRAME_PROBE: &str = "realistic_frame_metrics";

pub fn frame_metrics(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let mut payload = match run_probe_object(
        &config.project_root,
        &[
            "cargo",
            "build",
            "--release",
            "--quiet",
            "--bin",
            "frame_metrics",
        ],
        probe_path("frame_metrics"),
    ) {
        Ok(mut value) => {
            value["meta"]["generated_from"] = json!("rust:frame_metrics");
            value["meta"]["probe_status"] = json!("completed");
            value
        }
        Err(error) => json!({
            "meta": {"generated_from": "rust:frame_metrics", "probe_status": "failed", "error": error.to_string()},
            "scenarios": [],
        }),
    };

    add_realistic_frame_metrics(&mut payload, config);
    annotate_frame_scenarios(&mut payload);
    if payload["meta"]["realistic_probe"]["status"].as_str() == Some("failed") {
        payload["meta"]["probe_status"] = json!("failed");
        payload["meta"]["error"] = payload["meta"]["realistic_probe"]["error"].clone();
    }

    let failed = payload
        .get("meta")
        .and_then(|meta| meta.get("probe_status"))
        .and_then(Value::as_str)
        == Some("failed");
    shared::write_visibility(
        &config.output_dir.join("frame_metrics.json"),
        &payload,
        "frame metrics",
        render_frame_metrics(&payload),
    )?;
    if failed {
        bail!("frame metrics probe failed");
    }
    Ok(())
}

fn add_realistic_frame_metrics(payload: &mut Value, config: &LensConfig) {
    let status = if !probe_target_present(&config.project_root, REALISTIC_FRAME_PROBE) {
        json!({
            "status": "unavailable",
            "reason": format!("Scratchpad does not define a {REALISTIC_FRAME_PROBE} binary target"),
        })
    } else {
        match run_probe_object(
            &config.project_root,
            &[
                "cargo",
                "build",
                "--release",
                "--quiet",
                "--bin",
                REALISTIC_FRAME_PROBE,
            ],
            probe_path(REALISTIC_FRAME_PROBE),
        ) {
            Ok(realistic) => {
                append_realistic_scenarios(payload, realistic);
                json!({"status": "completed", "binary": REALISTIC_FRAME_PROBE})
            }
            Err(error) => json!({
                "status": "failed",
                "binary": REALISTIC_FRAME_PROBE,
                "error": error.to_string(),
            }),
        }
    };

    payload["meta"]["realistic_probe"] = status;
}

fn append_realistic_scenarios(payload: &mut Value, realistic: Value) {
    let Some(base_scenarios) = payload.get_mut("scenarios").and_then(Value::as_array_mut) else {
        return;
    };
    for mut scenario in realistic
        .get("scenarios")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        scenario["probe_binary"] = json!(REALISTIC_FRAME_PROBE);
        scenario.as_object_mut().map(|object| {
            object
                .entry("measurement_scope".to_string())
                .or_insert_with(|| json!("end_to_end_render_submission"))
        });
        base_scenarios.push(scenario);
    }
}

fn annotate_frame_scenarios(payload: &mut Value) {
    let Some(scenarios) = payload.get_mut("scenarios").and_then(Value::as_array_mut) else {
        return;
    };
    for scenario in scenarios {
        let realistic = scenario.get("probe_binary").and_then(Value::as_str)
            == Some(REALISTIC_FRAME_PROBE)
            || scenario
                .get("measurement_scope")
                .and_then(Value::as_str)
                .is_some_and(|scope| scope.starts_with("end_to_end"));

        if scenario.get("measurement_scope").is_none() {
            scenario["measurement_scope"] = json!(if realistic {
                "end_to_end_render_submission"
            } else {
                "measured_frame_path_cpu"
            });
        }
        if scenario.get("metric_role").is_none() {
            scenario["metric_role"] = json!(if realistic {
                "user_visible_frame_realism"
            } else {
                "theoretical_frame_production_capacity"
            });
        }
        if scenario.get("present_included").is_none() {
            scenario["present_included"] = json!(false);
        }
        if scenario.get("vsync").is_none() {
            scenario["vsync"] = json!(false);
        }
        if scenario.get("included_work").is_none() {
            scenario["included_work"] = json!(if realistic {
                vec![
                    "event/update boundary",
                    "app state update",
                    "layout/viewport computation",
                    "text shaping or glyph/cache lookup when the probe exercises it",
                    "paint command generation",
                    "GPU upload/render submission when available",
                ]
            } else {
                vec!["timed frame-path work reported by the frame_metrics probe"]
            });
        }
        if scenario.get("omitted_work").is_none() {
            scenario["omitted_work"] = json!(if realistic {
                vec![
                    "present callback unless present_included is true",
                    "vsync pacing unless vsync is true",
                ]
            } else {
                vec![
                    "full real-window event loop",
                    "GPU upload/render submission unless listed in phases",
                    "OS compositor/present callback",
                    "vsync pacing",
                ]
            });
        }

        if let Some(p99_ms) = scenario.get("p99_ms").and_then(Value::as_f64) {
            if p99_ms > 0.0 {
                scenario["theoretical_fps_p99"] = json!(1000.0 / p99_ms);
                scenario["refresh_budget_utilization"] = json!({
                    "60_hz": p99_ms / (1000.0 / 60.0),
                    "120_hz": p99_ms / (1000.0 / 120.0),
                    "144_hz": p99_ms / (1000.0 / 144.0),
                    "240_hz": p99_ms / (1000.0 / 240.0),
                });
            }
        }
    }
}

fn probe_target_present(project_root: &std::path::Path, bin: &str) -> bool {
    if project_root
        .join("src")
        .join("bin")
        .join(format!("{bin}.rs"))
        .exists()
    {
        return true;
    }
    let Ok(cargo_toml) = std::fs::read_to_string(project_root.join("Cargo.toml")) else {
        return false;
    };
    let Ok(manifest) = toml::from_str::<toml::Value>(&cargo_toml) else {
        return false;
    };
    manifest
        .get("bin")
        .and_then(toml::Value::as_array)
        .is_some_and(|bins| {
            bins.iter()
                .any(|entry| entry.get("name").and_then(toml::Value::as_str) == Some(bin))
        })
}

#[cfg(test)]
mod tests {
    use super::{annotate_frame_scenarios, append_realistic_scenarios, probe_target_present};
    use serde_json::json;

    #[test]
    fn annotates_existing_frame_probe_as_theoretical_cpu_path() {
        let mut payload = json!({
            "meta": {},
            "scenarios": [{
                "scenario_id": "ui_render_frame_120hz",
                "p99_ms": 1.0
            }]
        });

        annotate_frame_scenarios(&mut payload);
        let row = &payload["scenarios"][0];

        assert_eq!(row["measurement_scope"], json!("measured_frame_path_cpu"));
        assert_eq!(
            row["metric_role"],
            json!("theoretical_frame_production_capacity")
        );
        assert_eq!(row["present_included"], json!(false));
        assert_eq!(row["theoretical_fps_p99"], json!(1000.0));
        assert_eq!(row["refresh_budget_utilization"]["120_hz"], json!(0.12));
    }

    #[test]
    fn appends_realistic_probe_scenarios_with_end_to_end_scope() {
        let mut payload = json!({"meta": {}, "scenarios": []});
        let realistic = json!({
            "scenarios": [{
                "scenario_id": "scroll_real_window_end_to_end",
                "p99_ms": 6.25
            }]
        });

        append_realistic_scenarios(&mut payload, realistic);
        annotate_frame_scenarios(&mut payload);
        let row = &payload["scenarios"][0];

        assert_eq!(row["probe_binary"], json!("realistic_frame_metrics"));
        assert_eq!(
            row["measurement_scope"],
            json!("end_to_end_render_submission")
        );
        assert_eq!(row["metric_role"], json!("user_visible_frame_realism"));
        assert_eq!(row["refresh_budget_utilization"]["240_hz"], json!(1.5));
    }

    #[test]
    fn detects_explicit_or_implicit_realistic_probe_targets() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            r#"
                [package]
                name = "scratchpad"
                version = "0.1.0"
                edition = "2021"

                [[bin]]
                name = "realistic_frame_metrics"
                path = "tools/realistic_frame_metrics.rs"
            "#,
        )
        .unwrap();
        assert!(probe_target_present(dir.path(), "realistic_frame_metrics"));

        let implicit = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(implicit.path().join("src").join("bin")).unwrap();
        std::fs::write(
            implicit
                .path()
                .join("src")
                .join("bin")
                .join("realistic_frame_metrics.rs"),
            "fn main() {}",
        )
        .unwrap();
        assert!(probe_target_present(
            implicit.path(),
            "realistic_frame_metrics"
        ));
    }
}
