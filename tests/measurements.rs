use serde_json::{json, Value};
use std::process::Command;

#[test]
fn probes_use_cargo_executables_with_environment_or_configured_target_directories(
) -> anyhow::Result<()> {
    for via_environment in [true, false] {
        let root = tempfile::tempdir()?;
        std::fs::create_dir_all(root.path().join("src/bin"))?;
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2021'\n",
        )?;
        let report = json!({"meta":{},"scenarios":[{"scenario_id":"test", "frame_count":10,
            "p95_ms":1,"p99_ms":2,"budget_ms":8,"p99_budget_ms":12}]})
        .to_string();
        std::fs::write(
            root.path().join("src/bin/frame_metrics.rs"),
            format!("fn main() {{ print!(\"{{}}\", {report:?}); }}"),
        )?;
        let destination = root.path().join("relocated-build");
        std::fs::create_dir_all(root.path().join("target/release"))?;
        std::fs::write(
            root.path().join("target/release/frame_metrics"),
            "must never execute stale binary",
        )?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_splens"));
        command
            .args(["measure", "frame-metrics"])
            .current_dir(root.path())
            .env_remove("CARGO_TARGET_DIR");
        if via_environment {
            command.env("CARGO_TARGET_DIR", &destination);
        } else {
            std::fs::create_dir(root.path().join(".cargo"))?;
            std::fs::write(
                root.path().join(".cargo/config.toml"),
                "[build]\ntarget-dir='relocated-build'\n",
            )?;
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let payload: Value = serde_json::from_slice(&std::fs::read(
            root.path().join("target/analysis/frame_metrics.json"),
        )?)?;
        assert!(
            std::path::Path::new(payload["meta"]["probe_executable"].as_str().unwrap())
                .starts_with(&destination)
        );
        if via_environment {
            exercise_event_probes(root.path(), &destination)?;
        }
    }
    Ok(())
}

fn exercise_event_probes(root: &std::path::Path, target: &std::path::Path) -> anyhow::Result<()> {
    use std::fs;
    let event = |scenario: &str, workload: u64, status: &str| {
        json!({"scenario":scenario,"step_index":0,
        "workload_value":workload,"elapsed_ns":95_000_000,"status":status,"measurement_scope":"prepared_operation",
            "live_growth_bytes":-100,"retained_live_bytes_before_teardown":350,"process_resources":{"read_bytes":1024},
            "phase_elapsed_ns":{"retained_cache_bytes":4194304,"cache_limit_bytes":4194304}})
    };
    let mut partial = event("partial", 1, "ok");
    partial["elapsed_ns"] = json!(10);
    let active = |scenario| json!({"event":"step_started","scenario":scenario,"step_index":1,"workload_value":2});
    let mut arms = Vec::new();
    for (id, text, ending) in [
        ("near", event("near", 1, "ok").to_string(), ""),
        (
            "partial",
            format!("{partial}\n{{}}\n{}\n{{truncated", active("partial")),
            "std::process::exit(7);",
        ),
        (
            "timeout",
            active("timeout").to_string(),
            "std::thread::sleep(std::time::Duration::from_secs(60));",
        ),
        ("later", partial.to_string().replace("partial", "later"), ""),
    ] {
        arms.push(format!("{id:?} => {{println!(\"{{}}\",{text:?});std::io::Write::flush(&mut std::io::stdout()).unwrap();{ending}}}"));
    }
    let stub = |arms: Vec<String>| {
        format!("fn main() {{for id in std::env::args().skip(2).step_by(2) {{match id.as_str() {{{}, _ => panic!(\"unknown scenario\")}}}}}}",arms.join(","))
    };
    arms.push("\"omitted\" => {}".into());
    fs::write(root.join("src/bin/resource_probe.rs"), stub(arms))?;
    let definitions = [
        ("large_file_first_visible_ceiling", 1_073_741_824, "bytes"),
        (
            "large_file_background_index_ceiling",
            1_073_741_824,
            "bytes",
        ),
        ("in_memory_text_ingest_ceiling", 1_073_741_824, "bytes"),
        ("text_layout_ceiling", 134_217_728, "bytes"),
        ("many_file_first_visible_ceiling", 50_000, "files"),
        ("many_file_background_hydration_ceiling", 50_000, "files"),
        ("search_file_size_ceiling", 1_073_741_824, "bytes"),
        ("search_target_count_ceiling", 10_000, "files"),
        ("tab_count_ceiling", 10_000, "tabs"),
        ("split_count_ceiling", 1_000, "splits"),
        ("view_count_ceiling", 1_000, "views"),
        ("paste_size_ceiling", 134_217_728, "bytes"),
    ];
    let mut manifest = json!({"probes":{"resource_probe":["near","partial","timeout","later","omitted"]},"resource_budgets":{},"capacity_budgets":{}});
    for id in ["near", "partial", "timeout", "later"] {
        manifest["resource_budgets"][id] = json!({"metric":"/elapsed_ns","max":100_000_000});
    }
    let arms = definitions
        .into_iter()
        .map(|(id, workload, unit)| {
            manifest["capacity_budgets"][id] = json!({"threshold_ms":100});
            let lines = (0..3)
                .map(|repeat| {
                    let mut row = event(id, workload, "ok");
                    row["repeat_index"] = json!(repeat);
                    row["workload_unit"] = json!(unit);
                    row["expected_repetition_count"] = json!(3);
                    if id == "search_file_size_ceiling" {
                        row["measurement_scope"] = json!("prepared_search_component_completion");
                        row["result_value"] = json!(1);
                        row["measurement_contract"] = json!({"fixture_kind":"non_repeating","fixture_revision":2,
                            "production_path":true,"text_backing":"store","code_path":"search_target_ranges/production_worker_policy",
                            "search_fixture_revision":3,"document_bytes":workload,"expected_matches":1,"completion_endpoint":"all_ranges_returned"});
                    }
                    row.to_string()
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{id:?} => println!(\"{{}}\",{lines:?})")
        })
        .collect();
    fs::write(root.join("src/bin/capacity_probe.rs"), stub(arms))?;
    fs::create_dir_all(root.join("benches"))?;
    fs::write(
        root.join("benches/measurement_manifest.json"),
        manifest.to_string(),
    )?;
    for (tool, success, file) in [
        ("resources", false, "resource_profiles.json"),
        ("capacity", true, "capacity_report.json"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_splens"))
            .args(["measure", tool])
            .current_dir(root)
            .env("CARGO_TARGET_DIR", target)
            .env("SPLENS_PROBE_TIMEOUT_SECS", "1")
            .output()?;
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: Value =
            serde_json::from_slice(&fs::read(root.join("target/analysis").join(file))?)?;
        let rows = report["scenarios"].as_array().unwrap();
        if success {
            assert_eq!(rows.len(), 12);
            assert!(rows
                .iter()
                .all(|row| row["samples"][0]["repetition_count"] == 9
                    && row["samples"][0]["complete"] == true));
            let search = rows
                .iter()
                .find(|row| row["scenario"] == "search_file_size_ceiling")
                .unwrap();
            assert_eq!(search["samples"][0]["result_value"], 1);
            assert_eq!(search["within_budget_workload"], 1_073_741_824);
        } else {
            assert_eq!(report["meta"]["probe_status"], "partial");
            assert_eq!(rows.len(), 4);
            assert_eq!(report["meta"]["missing_scenarios"], json!(["omitted"]));
            let near = rows.iter().find(|row| row["scenario"] == "near").unwrap();
            assert_eq!(near["samples"].as_array().unwrap().len(), 3);
            let sample = &near["samples"][0];
            assert_eq!(sample["retained_live_bytes_before_teardown"], 350);
            assert_eq!(sample["live_growth_bytes"], -100);
            assert_eq!(sample["process_resources"]["read_bytes"], 1024);
            assert_eq!(sample["cache_bound_held"], true);
            for id in ["partial", "timeout"] {
                assert!(
                    rows.iter().find(|row| row["scenario"] == id).unwrap()["samples"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|sample| sample["status"] == "error")
                );
            }
        }
    }
    manifest["probes"]["resource_probe"] = json!(["near", "later"]);
    manifest["probe_fixture_groups"] =
        json!({"resource_probe":[{"id":"shared","scenarios":["near","later"]}]});
    manifest["resource_budgets"]["later"]["min_independent_launches"] = json!(4);
    fs::write(
        root.join("benches/measurement_manifest.json"),
        manifest.to_string(),
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_splens"))
        .args(["measure", "resources"])
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&fs::read(
        root.join("target/analysis/resource_profiles.json"),
    )?)?;
    assert_eq!(report["meta"]["probe_status"], "completed");
    for row in report["scenarios"].as_array().unwrap() {
        let samples = row["samples"].as_array().unwrap();
        assert_eq!(samples.len(), 4);
        for (index, sample) in samples.iter().enumerate() {
            assert_eq!(sample["launch_index"], index);
            assert_eq!(sample["independent_launch_count"], 4);
            assert_eq!(sample["shared_fixture_group"], "shared");
            assert_eq!(sample["launch_scenarios"], json!(["near", "later"]));
            assert_eq!(
                sample["launch_measurement_quality"]["scope"],
                "independent_probe_launch_including_setup"
            );
        }
    }
    Ok(())
}

#[test]
fn unverified_criterion_cannot_supply_evidence_or_pass_fail_on_slow() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let estimates = root
        .path()
        .join("target/criterion/search_active_file_size/16384/new");
    std::fs::create_dir_all(&estimates)?;
    std::fs::write(estimates.join("estimates.json"), "{truncated")?;
    std::fs::create_dir(root.path().join("benches"))?;
    std::fs::write(
        root.path().join("benches/search_benchmark_targets.json"),
        json!({
            "search_active_file_size": {"workload_family":"search", "threshold_ms":50}
        })
        .to_string(),
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_splens"))
        .args(["measure", "search", "--skip-bench", "--fail-on-slow"])
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(root.path())
        .output()?;
    assert!(!output.status.success());
    let rows: Value = serde_json::from_slice(&std::fs::read(
        root.path().join("target/analysis/search_speed.json"),
    )?)?;
    assert_eq!(rows, json!([]));
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("no current verified Criterion run manifest"));
    let provenance: Value = serde_json::from_slice(&std::fs::read(
        root.path()
            .join("target/analysis/search_speed.json.provenance.json"),
    )?)?;
    assert_eq!(provenance["status"], "failed");
    Ok(())
}
