use super::common::{run_probe_object, write_probe_artifact};
use super::render::render_frame_metrics;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use anyhow::Result;
use serde_json::{json, Value};

const REALISTIC_FRAME_PROBE: &str = "realistic_frame_metrics";

pub fn frame_metrics(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let mut payload = match run_probe_object(&config.project_root, "frame_metrics") {
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
    if !payload["scenarios"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty() && rows.iter().all(super::common::valid_frame))
    {
        payload["meta"]["probe_status"] = json!("failed");
        payload["meta"]["error"] =
            json!("frame probe returned empty, invalid, or unspecified-scope measurements");
    }
    if payload["meta"]["realistic_probe"]["status"].as_str() == Some("failed") {
        payload["meta"]["probe_status"] = json!("failed");
        payload["meta"]["error"] = payload["meta"]["realistic_probe"]["error"].clone();
    }

    let failure = (payload["meta"]["probe_status"].as_str() == Some("failed"))
        .then(|| "frame metrics probe failed".to_string());
    write_probe_artifact(
        &config.output_dir.join("frame_metrics.json"),
        &payload,
        "frame metrics",
        render_frame_metrics(&payload),
        failure,
    )
}

fn add_realistic_frame_metrics(payload: &mut Value, config: &LensConfig) {
    let status = if !probe_target_present(&config.project_root, REALISTIC_FRAME_PROBE) {
        json!({
            "status": "unavailable",
            "reason": format!("Scratchpad does not define a {REALISTIC_FRAME_PROBE} binary target"),
        })
    } else {
        match run_probe_object(&config.project_root, REALISTIC_FRAME_PROBE) {
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

fn append_realistic_scenarios(payload: &mut Value, mut realistic: Value) {
    let Some(base_scenarios) = payload.get_mut("scenarios").and_then(Value::as_array_mut) else {
        return;
    };
    let Some(scenarios) = realistic.get_mut("scenarios").and_then(Value::as_array_mut) else {
        return;
    };
    for mut scenario in scenarios.drain(..) {
        if let Some(object) = scenario.as_object_mut() {
            object.insert("probe_binary".into(), json!(REALISTIC_FRAME_PROBE));
            object
                .entry("measurement_scope")
                .or_insert_with(|| json!("unknown"));
        }
        // Preserve malformed rows so validation fails instead of hiding bad evidence.
        base_scenarios.push(scenario);
    }
}

fn annotate_frame_scenarios(payload: &mut Value) {
    let Some(scenarios) = payload.get_mut("scenarios").and_then(Value::as_array_mut) else {
        return;
    };
    scenarios.iter_mut().for_each(annotate_frame_scenario);
}

fn annotate_frame_scenario(scenario: &mut Value) {
    let realistic = scenario.get("probe_binary").and_then(Value::as_str)
        == Some(REALISTIC_FRAME_PROBE)
        || scenario
            .get("measurement_scope")
            .and_then(Value::as_str)
            .is_some_and(|scope| scope.starts_with("end_to_end"));
    let p99_ms = scenario
        .get("p99_ms")
        .and_then(Value::as_f64)
        .filter(|value| *value > 0.0);
    let Some(fields) = scenario.as_object_mut() else {
        return;
    };
    let (scope, role, included, omitted) = frame_defaults(realistic);
    for (key, value) in [
        ("measurement_scope", json!(scope)),
        ("metric_role", json!(role)),
        ("present_included", json!(false)),
        ("percentile_method", json!("unknown_legacy_probe")),
        ("percentile_resolution_ms", Value::Null),
        ("vsync", json!(false)),
        ("included_work", included),
        ("omitted_work", omitted),
    ] {
        fields.entry(key.to_string()).or_insert(value);
    }
    if let Some(p99_ms) = p99_ms {
        fields.insert("theoretical_fps_p99".to_string(), json!(1000.0 / p99_ms));
        fields.insert(
            "refresh_budget_utilization".to_string(),
            json!({
                "60_hz": p99_ms / (1000.0 / 60.0),
                "120_hz": p99_ms / (1000.0 / 120.0),
                "144_hz": p99_ms / (1000.0 / 144.0),
                "240_hz": p99_ms / (1000.0 / 240.0),
            }),
        );
    }
}

fn frame_defaults(realistic: bool) -> (&'static str, &'static str, Value, Value) {
    if realistic {
        (
            "unknown",
            "render_preparation_scope_unspecified",
            json!([]),
            json!([
                "present callback unless present_included is true",
                "vsync pacing unless vsync is true",
            ]),
        )
    } else {
        (
            "measured_frame_path_cpu",
            "theoretical_frame_production_capacity",
            json!(["timed frame-path work reported by the frame_metrics probe"]),
            json!([
                "full real-window event loop",
                "GPU upload/render submission unless listed in phases",
                "OS compositor/present callback",
                "vsync pacing",
            ]),
        )
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
    fn frame_annotation_preserves_explicit_scope_and_invalid_evidence() {
        let mut payload = json!({"scenarios":[{"p99_ms":1.0}]});
        append_realistic_scenarios(
            &mut payload,
            json!({"scenarios":[
                {"p99_ms":6.25}, {"p99_ms":0, "measurement_scope":"end_to_end", "present_included":true}, 42
            ]}),
        );
        annotate_frame_scenarios(&mut payload);
        let rows = &payload["scenarios"];
        assert_eq!(rows[0]["measurement_scope"], "measured_frame_path_cpu");
        assert_eq!(
            rows[0]["metric_role"],
            "theoretical_frame_production_capacity"
        );
        assert_eq!(rows[0]["present_included"], false);
        assert_eq!(rows[0]["theoretical_fps_p99"], 1000.0);
        assert_eq!(rows[0]["refresh_budget_utilization"]["120_hz"], 0.12);
        assert_eq!(rows[1]["probe_binary"], "realistic_frame_metrics");
        assert_eq!(rows[1]["measurement_scope"], "unknown");
        assert_eq!(
            rows[1]["metric_role"],
            "render_preparation_scope_unspecified"
        );
        assert_eq!(rows[1]["refresh_budget_utilization"]["240_hz"], 1.5);
        assert_eq!(rows[2]["measurement_scope"], "end_to_end");
        assert_eq!(rows[2]["present_included"], true);
        assert!(rows[2]["theoretical_fps_p99"].is_null());
        assert_eq!(rows[3], 42);
        for invalid in [json!(null), json!({"scenarios":false})] {
            append_realistic_scenarios(&mut payload, invalid.clone());
            let mut invalid = invalid;
            append_realistic_scenarios(&mut invalid, json!({"scenarios":[]}));
            annotate_frame_scenarios(&mut invalid);
        }
        assert_eq!(payload["scenarios"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn probe_discovery_handles_missing_malformed_explicit_and_implicit_targets(
    ) -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        assert!(!probe_target_present(dir.path(), "realistic_frame_metrics"));
        for (manifest, found) in [
            ("{", false),
            ("[package]\nname='fixture'", false),
            (
                "[[bin]]\nname='realistic_frame_metrics'\npath='tools/probe.rs'",
                true,
            ),
        ] {
            std::fs::write(dir.path().join("Cargo.toml"), manifest)?;
            assert_eq!(
                probe_target_present(dir.path(), "realistic_frame_metrics"),
                found
            );
        }
        std::fs::create_dir_all(dir.path().join("src/bin"))?;
        std::fs::write(dir.path().join("src/bin/implicit.rs"), "fn main() {}")?;
        assert!(probe_target_present(dir.path(), "implicit"));
        Ok(())
    }
}
