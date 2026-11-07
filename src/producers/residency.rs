use crate::{
    cli::MeasureOptions,
    config::LensConfig,
    residency::{LadderArtifact, Plan, Request, Trial},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Deserialize)]
struct Input {
    source_context: Value,
    trials: Vec<Trial>,
}

#[derive(Serialize)]
struct Archive<'a> {
    source_context: &'a Value,
    trials: &'a [Trial],
}

pub fn import(config: &LensConfig, path: &Path, selected: &[String]) -> Result<()> {
    let input: Input = serde_json::from_slice(&std::fs::read(path)?)?;
    let context = crate::provenance::context(&config.project_root);
    ensure!(
        crate::provenance::same_source(&input.source_context, &context),
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
    let plan = Plan::new(&manifest, &options.scenarios)?;
    let probe = plan
        .config
        .probe
        .as_deref()
        .context("missing residency probe target")?;
    ensure!(
        !probe.is_empty() && probe.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "invalid probe name"
    );
    if !config
        .project_root
        .join(format!("src/bin/{probe}.rs"))
        .is_file()
    {
        write(config, &plan.analyze(Vec::new())?)?;
        anyhow::bail!(
            "project has not implemented {probe}; missing rungs recorded, no timings inferred"
        );
    }
    let executable = super::common::build_probe(&config.project_root, probe)?;
    let source = crate::provenance::compute_context(&config.project_root);
    let archive = config.output_dir.join("residency-raw");
    std::fs::create_dir_all(&archive)?;
    let mut trials = Vec::new();
    for (sequence, request) in plan.requests().enumerate() {
        let prefix = archive.join(format!("{}-{}", std::process::id(), sequence + 1));
        trials.push(run_trial(config, &executable, &source, &request, &prefix)?);
        std::fs::write(
            archive.join("trials.json"),
            serde_json::to_vec_pretty(&Archive {
                source_context: &source,
                trials: &trials,
            })?,
        )?;
    }
    ensure!(
        crate::provenance::same_source(
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
    let mut artifact = plan.analyze(trials)?;
    artifact.source_context = source;
    write(config, &artifact)
}

fn run_trial(
    config: &LensConfig,
    executable: &Path,
    source: &Value,
    request: &Request<'_>,
    prefix: &Path,
) -> Result<Trial> {
    let (id, size, fixture, profile) = request.key;
    let stdout = prefix.with_extension("json");
    let mut command = Command::new(executable);
    command
        .current_dir(&config.project_root)
        .args([
            "--scenario",
            id,
            "--workload",
            &size.to_string(),
            "--fixture",
            fixture,
            "--rung",
            request.rung,
            "--launch",
            &request.launch.to_string(),
        ])
        .env_remove("SCRATCHPAD_MEMORY_PROFILE");
    if let Some(bytes) = profile {
        command.env("SCRATCHPAD_MEMORY_PROFILE", bytes.to_string());
    }
    command
        .env(
            "SCRATCHPAD_DIAGNOSTIC_REVISION",
            source["revision"].as_str().unwrap_or(""),
        )
        .stdout(Stdio::from(std::fs::File::create(&stdout)?))
        .stderr(Stdio::from(std::fs::File::create(
            prefix.with_extension("stderr"),
        )?));
    let before = crate::environment::control();
    let exit = crate::process::run(&mut command, Duration::from_secs(300))?;
    let after = crate::environment::control();
    ensure!(
        exit.status.success() && !exit.timed_out,
        "residency trial failed; raw output at {}",
        stdout.display()
    );
    let mut trial: Trial = serde_json::from_slice(&std::fs::read(&stdout)?)?;
    ensure!(
        request.matches(&trial),
        "probe returned different requested trial"
    );
    trial.measurement_quality = crate::environment::quality(&before, &after);
    Ok(trial)
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
    use super::{import, measure, Archive};
    use crate::{cli::MeasureOptions, config::LensConfig};
    use anyhow::{ensure, Result};
    use serde_json::{json, Value};
    use std::process::Command;

    #[test]
    fn residency_runner_archives_requests_and_rejects_stale_or_mismatched_evidence() -> Result<()> {
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
            serde_json::to_vec(&Archive {
                source_context: &context,
                trials: &trials,
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
        let options = || MeasureOptions {
            skip_bench: false,
            fail_on_slow: false,
            index_only: false,
            scenarios: Vec::new(),
        };
        let result = measure(&config, options());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("has not implemented residency_probe"));
        let value: Value = serde_json::from_slice(&std::fs::read(&artifact)?)?;
        assert_eq!(value["complete"], false);
        assert_eq!(value["trials"], json!([]));
        assert!(!value["gaps"].as_array().unwrap().is_empty());
        std::fs::create_dir_all(root.path().join("src/bin"))?;
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )?;
        let template = serde_json::to_string(&crate::residency::tests::fixture().1[0])?;
        let source = r##"fn main() {
            let args: Vec<_> = std::env::args().skip(1).collect();
            let rung = &args[7];
            let profile = std::env::var("SCRATCHPAD_MEMORY_PROFILE").unwrap_or("null".into());
            let mut trial = TEMPLATE.to_owned()
                .replace("owned_contiguous", rung)
                .replace("fixture-revision", &std::env::var("SCRATCHPAD_DIAGNOSTIC_REVISION").unwrap())
                .replace("\"memory_profile_bytes\":4294967296", &format!("\"memory_profile_bytes\":{profile}"))
                .replace("\"launch_index\":0", &format!("\"launch_index\":{}", args[9]));
            if rung == "store_warm" {
                trial = trial.replace("\"store_read_count\":0", "\"store_read_count\":1");
            }
            if std::path::Path::new("break_probe").exists() {
                trial = trial.replace("\"workload_value\":64", "\"workload_value\":65");
            }
            println!("{trial}");
        }"##.replace("TEMPLATE", &format!("{template:?}"));
        std::fs::write(root.path().join("src/bin/residency_probe.rs"), source)?;
        measure(&config, options())?;
        let value: Value = serde_json::from_slice(&std::fs::read(&artifact)?)?;
        assert_eq!(value["complete"], true);
        assert_eq!(value["promise_eligible"], false);
        let archive = config.output_dir.join("residency-raw/trials.json");
        let raw: Value = serde_json::from_slice(&std::fs::read(archive)?)?;
        assert_eq!(raw["trials"].as_array().unwrap().len(), 45);
        for (index, profile) in [
            (0, json!(4294967296u64)),
            (15, json!(17179869184u64)),
            (30, Value::Null),
        ] {
            assert_eq!(raw["trials"][index]["memory_profile_bytes"], profile);
            assert_eq!(raw["trials"][index + 5]["rung"], "owned_piece_tree");
            assert!(raw["trials"][index]["measurement_quality"]["before"].is_object());
        }
        std::fs::write(root.path().join("break_probe"), "")?;
        assert!(measure(&config, options())
            .unwrap_err()
            .to_string()
            .contains("different requested trial"));
        Ok(())
    }
}
