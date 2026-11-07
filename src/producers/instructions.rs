//! Optional whole-process Callgrind companion. Setup is included unless the
//! probe supplies client instrumentation; wall time here is never gate evidence.
use anyhow::{ensure, Context, Result};
use clap::Parser;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long, value_parser = ["resource_probe", "capacity_probe"])]
    pub probe: String,
    #[arg(long = "scenario", required = true)]
    pub scenarios: Vec<String>,
    #[arg(long, default_value_t = 3600)]
    pub timeout_seconds: u64,
}

pub fn parse(text: &str) -> Result<u64> {
    let events = text
        .lines()
        .find_map(|l| l.strip_prefix("events: "))
        .context("missing Callgrind events")?;
    let index = events
        .split_whitespace()
        .position(|v| v == "Ir")
        .context("missing instruction event")?;
    // Multiple dumps/threads must be aggregated explicitly, not silently selected.
    let totals: Vec<_> = text
        .lines()
        .filter_map(|l| l.strip_prefix("summary: "))
        .collect();
    ensure!(totals.len() == 1, "expected one Callgrind summary");
    let count = totals[0]
        .split_whitespace()
        .nth(index)
        .context("missing Ir summary")?
        .parse::<u64>()?;
    ensure!(count > 0, "empty instruction profile");
    Ok(count)
}

pub fn measure(args: Args) -> Result<()> {
    let config = crate::config::LensConfig::load(args.config.as_deref())?;
    let manifest = crate::outcomes::load(&config.project_root)?;
    ensure!(args.timeout_seconds > 0, "timeout must be positive");
    ensure!(
        args.scenarios
            .iter()
            .all(|id| manifest["probes"][&args.probe]
                .as_array()
                .is_some_and(|ids| ids.contains(&json!(id)))),
        "unknown instruction scenario"
    );
    let executable = super::common::build_probe(&config.project_root, &args.probe)?;
    let source = crate::provenance::compute_context(&config.project_root);
    let directory = config.output_dir.join(format!(
        "instructions-{}-{}",
        std::process::id(),
        crate::provenance::timestamp() as u64
    ));
    std::fs::create_dir_all(&directory)?;
    let output = directory.join("callgrind.out");
    let mut command = Command::new("valgrind");
    command
        .current_dir(&config.project_root)
        .args([
            "--tool=callgrind",
            "--error-exitcode=99",
            "--collect-jumps=yes",
        ])
        .arg(format!("--callgrind-out-file={}", output.display()))
        .arg(&executable);
    for id in &args.scenarios {
        command.args(["--scenario", id]);
    }
    command
        .stdout(Stdio::from(std::fs::File::create(
            directory.join("stdout.jsonl"),
        )?))
        .stderr(Stdio::from(std::fs::File::create(
            directory.join("stderr.txt"),
        )?));
    let exit = crate::process::run(&mut command, Duration::from_secs(args.timeout_seconds))?;
    ensure!(
        exit.status.success() && !exit.timed_out,
        "Callgrind failed or timed out; raw logs: {}",
        directory.display()
    );
    let count = parse(&std::fs::read_to_string(&output)?)?;
    let after = crate::provenance::compute_context(&config.project_root);
    ensure!(
        source["revision"] == after["revision"]
            && source["source_fingerprint"] == after["source_fingerprint"],
        "source changed during instrumentation"
    );
    let payload: Value = json!({"version":1,"diagnostic_only":true,"promise_eligible":false,"source_context":source,
        "probe":args.probe,"scenarios":args.scenarios,"instruction_count":count,"event":"Ir",
        "scope":"whole_process_including_setup_and_teardown","phase_counts":null,
        "phase_status":"requires_probe_client_instrumentation","native_latency_ns":null,"raw_profile":output});
    crate::shared::write_visibility(
        &config.output_dir.join("instruction_counts.json"),
        &payload,
        "instruction counts",
        format!("Whole-process instructions (including setup): {count}. Not latency evidence."),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn parser_requires_unambiguous_instruction_counts() {
        assert_eq!(
            super::parse("events: Dr Ir\nsummary: 10 123\n").unwrap(),
            123
        );
        for text in [
            "events: Dr\nsummary: 1\n",
            "events: Ir\nsummary: 0\n",
            "events: Ir\nsummary: 1\nsummary: 2\n",
            "events: Ir\n",
        ] {
            assert!(super::parse(text).is_err());
        }
    }
}
