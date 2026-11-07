use crate::{
    cli::MeasureOptions,
    config::LensConfig,
    residency::{LadderArtifact, Trial},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Deserialize, Serialize)]
pub struct Input {
    pub source_context: Value,
    pub trials: Vec<Trial>,
}

fn matching_source(recorded: &Value, current: &Value) -> bool {
    ["revision", "source_fingerprint"]
        .iter()
        .all(|k| recorded[*k].is_string() && recorded[*k] == current[*k])
}

pub fn import(config: &LensConfig, path: &Path, selected: &[String]) -> Result<()> {
    let input: Input = serde_json::from_slice(&std::fs::read(path)?)?;
    let context = crate::provenance::context(&config.project_root);
    ensure!(
        matching_source(&input.source_context, &context),
        "diagnostics do not match current source revision/fingerprint"
    );
    let manifest = crate::outcomes::load(&config.project_root)?;
    ensure!(
        input
            .trials
            .iter()
            .all(|t| Some(t.revision.as_str()) == context["revision"].as_str()),
        "trial revision differs from recorded source"
    );
    let mut artifact = crate::residency::analyze(&manifest, input.trials, selected)?;
    artifact.source_context = input.source_context;
    std::fs::create_dir_all(&config.output_dir)?;
    write(config, &artifact)?;
    crate::provenance::record(
        config,
        "residency_ladder.json",
        crate::provenance::timestamp(),
        &context,
        true,
    )?;
    Ok(())
}

pub fn measure(config: &LensConfig, options: MeasureOptions) -> Result<()> {
    let manifest = crate::outcomes::load(&config.project_root)?;
    // Validate selection and configuration before building or launching anything.
    let empty = crate::residency::analyze(&manifest, Vec::new(), &options.scenarios)?;
    let spec = &manifest["diagnostics"]["residency"];
    let probe = spec["probe"]
        .as_str()
        .context("missing residency probe target")?;
    ensure!(
        probe.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "invalid probe name"
    );
    if !config
        .project_root
        .join(format!("src/bin/{probe}.rs"))
        .is_file()
    {
        write(config, &empty)?;
        anyhow::bail!(
            "project has not implemented {probe}; missing rungs recorded, no timings inferred"
        );
    }
    let executable = super::common::build_probe(&config.project_root, probe)?;
    let source = crate::provenance::compute_context(&config.project_root);
    let archive = config.output_dir.join("residency-raw");
    std::fs::create_dir_all(&archive)?;
    let mut trials = Vec::new();
    let mut sequence = 0;
    for (id, op) in spec["operations"].as_object().unwrap() {
        if !options.scenarios.is_empty() && !options.scenarios.contains(id) {
            continue;
        }
        for size in op["workloads"].as_array().unwrap() {
            for fixture in op["fixtures"].as_array().unwrap() {
                for profile in spec["memory_profiles"].as_array().unwrap() {
                    for launch in 0..spec["independent_launches"].as_u64().unwrap() {
                        // Rotate order between independent launches, keeping five arms paired.
                        for offset in 0..5 {
                            let rung = crate::residency::RUNGS[(offset + launch as usize) % 5];
                            sequence += 1;
                            let prefix = archive.join(format!("{}-{sequence}", std::process::id()));
                            let stdout = prefix.with_extension("json");
                            let stderr = prefix.with_extension("stderr");
                            let mut command = Command::new(&executable);
                            command.current_dir(&config.project_root).args([
                                "--scenario",
                                id,
                                "--workload",
                                &size.to_string(),
                                "--fixture",
                                fixture.as_str().unwrap(),
                                "--rung",
                                rung,
                                "--launch",
                                &launch.to_string(),
                            ]);
                            command.env_remove("SCRATCHPAD_MEMORY_PROFILE");
                            if let Some(bytes) = profile.as_u64() {
                                command.env("SCRATCHPAD_MEMORY_PROFILE", bytes.to_string());
                            }
                            command.env(
                                "SCRATCHPAD_DIAGNOSTIC_REVISION",
                                source["revision"].as_str().unwrap_or(""),
                            );
                            command
                                .stdout(Stdio::from(std::fs::File::create(&stdout)?))
                                .stderr(Stdio::from(std::fs::File::create(&stderr)?));
                            let before = crate::environment::control();
                            let exit = crate::process::run(&mut command, Duration::from_secs(300))?;
                            let after = crate::environment::control();
                            ensure!(
                                exit.status.success() && !exit.timed_out,
                                "residency trial failed; raw output at {}",
                                stdout.display()
                            );
                            let mut trial: Trial =
                                serde_json::from_slice(&std::fs::read(&stdout)?)?;
                            ensure!(
                                trial.scenario == *id
                                    && trial.workload_value == size.as_u64().unwrap()
                                    && trial.fixture == fixture.as_str().unwrap()
                                    && trial.rung == rung
                                    && trial.launch_index == launch
                                    && json!(trial.memory_profile_bytes) == *profile,
                                "probe returned different requested trial"
                            );
                            trial.measurement_quality =
                                crate::environment::quality(&before, &after);
                            trials.push(trial);
                            std::fs::write(
                                archive.join("trials.json"),
                                serde_json::to_vec_pretty(&Input {
                                    source_context: source.clone(),
                                    trials: trials.clone(),
                                })?,
                            )?;
                        }
                    }
                }
            }
        }
    }
    ensure!(
        matching_source(
            &source,
            &crate::provenance::compute_context(&config.project_root)
        ),
        "source changed during diagnostics"
    );
    ensure!(
        trials
            .iter()
            .all(|t| Some(t.revision.as_str()) == source["revision"].as_str()),
        "diagnostic revision mismatch"
    );
    let mut artifact = crate::residency::analyze(&manifest, trials, &options.scenarios)?;
    artifact.source_context = source;
    write(config, &artifact)
}

fn write(config: &LensConfig, artifact: &LadderArtifact) -> Result<()> {
    let mut text = String::from("Residency diagnostics (cannot satisfy promise gates)\n");
    for group in &artifact.groups {
        text.push_str(&format!(
            "{} / {} / {} / {}: {}\n",
            group["scenario"],
            group["workload_value"],
            group["fixture"],
            group["memory_profile_bytes"],
            group["classification"]
        ));
    }
    crate::shared::write_visibility(
        &config.output_dir.join("residency_ladder.json"),
        &serde_json::to_value(artifact)?,
        "residency ladder",
        text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_bind_source_and_missing_probe_preserves_explicit_gaps() -> Result<()> {
        let root = tempfile::tempdir()?;
        std::fs::create_dir(root.path().join("benches"))?;
        let (mut manifest, mut trials) = crate::residency::tests::fixture();
        manifest["outcome_contract_version"] = json!(1);
        manifest["resource_budgets"] = json!({});
        manifest["probe_measurements"] = json!({});
        manifest["capacity_budgets"]["search"]["outcome"] = json!({
            "promise":"search", "user_outcome":"All ranges returned", "endpoint":"all_ranges_returned",
            "blocking":"background_completion", "residency":"store_uncontrolled",
            "durability_at_endpoint":"not_required", "workload":"64 bytes"});
        manifest["diagnostics"]["residency"]["probe"] = json!("residency_probe");
        std::fs::write(
            root.path().join("benches/measurement_manifest.json"),
            serde_json::to_vec(&manifest)?,
        )?;
        std::fs::write(root.path().join(".gitignore"), "target/\n")?;
        for args in [
            vec!["init"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "-m",
                "fixture",
            ],
        ] {
            ensure!(
                Command::new("git")
                    .args(args)
                    .current_dir(root.path())
                    .output()?
                    .status
                    .success(),
                "git fixture"
            );
        }
        let config = LensConfig {
            project_name: "fixture".into(),
            project_root: root.path().into(),
            output_dir: root.path().join("target/analysis"),
        };
        std::fs::create_dir_all(&config.output_dir)?;
        let context = crate::provenance::context(root.path());
        for t in &mut trials {
            t.revision = context["revision"].as_str().unwrap().into();
        }
        let input = config.output_dir.join("input.json");
        std::fs::write(
            &input,
            serde_json::to_vec(&Input {
                source_context: context,
                trials,
            })?,
        )?;
        import(&config, &input, &[])?;
        let artifact = config.output_dir.join("residency_ladder.json");
        let value: Value = serde_json::from_slice(&std::fs::read(&artifact)?)?;
        assert_eq!(value["complete"], true);
        assert_eq!(value["promise_eligible"], false);
        assert_eq!(
            value["source_context"]["revision"],
            crate::provenance::context(root.path())["revision"]
        );
        assert!(crate::provenance::sidecar(&artifact).exists());
        std::fs::write(root.path().join("changed.rs"), "changed source")?;
        assert!(import(&config, &input, &[]).is_err());
        let result = measure(
            &config,
            MeasureOptions {
                skip_bench: false,
                fail_on_slow: false,
                index_only: false,
                scenarios: Vec::new(),
            },
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("has not implemented residency_probe"));
        let value: Value = serde_json::from_slice(&std::fs::read(&artifact)?)?;
        assert_eq!(value["complete"], false);
        assert_eq!(value["trials"], json!([]));
        assert!(!value["gaps"].as_array().unwrap().is_empty());
        Ok(())
    }
}
