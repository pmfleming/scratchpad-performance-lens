use anyhow::{bail, ensure, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Debug, Args)]
pub(crate) struct MeasureLargeDirtySaveArgs {
    /// Explicit, already-built Scratchpad resource_probe executable.
    #[arg(long)]
    binary: PathBuf,
    /// New archive directory; existing paths are never overwritten.
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u8).range(1..=20))]
    launches: u8,
}

#[derive(Debug, Serialize)]
struct Summary {
    launch: u8,
    median_save_ms: f64,
    median_write_bytes: Option<u64>,
    write_bytes_available: bool,
}

#[derive(Deserialize)]
struct Sample {
    scenario: String,
    step_index: usize,
    status: String,
    phase_elapsed_ns: Phases,
    process_resources: Resources,
}

#[derive(Deserialize)]
struct Phases {
    save_ns: f64,
}

#[derive(Deserialize)]
struct Resources {
    write_bytes: Option<u64>,
}

pub(crate) fn measure_large_dirty_save(args: MeasureLargeDirtySaveArgs) -> Result<()> {
    measure(&args, Duration::from_secs(300))
}

fn measure(args: &MeasureLargeDirtySaveArgs, timeout: Duration) -> Result<()> {
    measure_with(args, timeout, "--large-dirty-save", summarize)
}

pub(super) fn measure_with<T: Serialize>(
    args: &MeasureLargeDirtySaveArgs,
    timeout: Duration,
    flag: &str,
    summarize: impl Fn(&str, u8) -> Result<T>,
) -> Result<()> {
    ensure!(
        (1..=20).contains(&args.launches),
        "launches must be between 1 and 20"
    );
    let binary = args
        .binary
        .canonicalize()
        .with_context(|| format!("resolve probe {}", args.binary.display()))?;
    ensure!(
        binary.is_file(),
        "probe must be a file: {}",
        binary.display()
    );
    // Keep the original SHA-256 identity rather than substituting the lens's
    // noncryptographic provenance fingerprint. Stream large executables.
    let mut hash = Sha256::new();
    std::io::copy(&mut File::open(&binary)?, &mut hash)?;
    let digest = format!("{:x}", hash.finalize());
    if let Some(parent) = args
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&args.output)
        .with_context(|| format!("create new archive {}", args.output.display()))?;
    write_json(
        &args.output.join("metadata.json"),
        &json!({
            "utc": OffsetDateTime::now_utc().format(&Rfc3339)?,
            "binary": binary,
            "binary_sha256": digest,
            "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            "launches": args.launches,
            "probe_flag": flag,
            "io_definition": "process write_bytes delta; WAL plus automatic checkpoint writes, not device traffic",
        }),
    )?;
    let mut summaries = Vec::new();
    for launch in 0..args.launches {
        let stdout = args.output.join(format!("run-{launch}.jsonl"));
        let stderr = args.output.join(format!("run-{launch}.stderr"));
        run_probe(&binary, &stdout, &stderr, timeout, flag).with_context(|| {
            format!("launch {launch}; raw evidence in {}", args.output.display())
        })?;
        let samples = fs::read_to_string(&stdout)?;
        summaries.push(summarize(&samples, launch).with_context(|| stdout.display().to_string())?);
    }
    // Publish a summary only after every independent launch succeeds.
    write_json(&args.output.join("summary.json"), &summaries)?;
    println!("{}", serde_json::to_string_pretty(&summaries)?);
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = File::create_new(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    Ok(())
}

fn run_probe(
    binary: &Path,
    stdout: &Path,
    stderr: &Path,
    timeout: Duration,
    flag: &str,
) -> Result<()> {
    // Direct-to-file output cannot deadlock on a full pipe and preserves evidence
    // on failure/timeout without buffering an unbounded subprocess in memory.
    let mut child = Command::new(binary)
        .arg(flag)
        .stdin(Stdio::null())
        .stdout(File::create_new(stdout)?)
        .stderr(File::create_new(stderr)?)
        .spawn()
        .with_context(|| format!("start {}", binary.display()))?;
    let started = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(error.into()),
            Ok(None) if started.elapsed() >= timeout => {
                break Err(anyhow::anyhow!(
                    "probe timed out after {} seconds",
                    timeout.as_secs_f64()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    };
    // Reap the process on every error path. No background probe should outlive a
    // failed run and contaminate subsequent measurements.
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let status = result?;
    ensure!(
        status.success(),
        "probe exited with {status}; see {}",
        stderr.display()
    );
    Ok(())
}

fn summarize(text: &str, launch: u8) -> Result<Summary> {
    let samples: Vec<Sample> = text
        .lines()
        .enumerate()
        .map(|(index, line)| {
            serde_json::from_str(line).with_context(|| format!("invalid probe line {}", index + 1))
        })
        .collect::<Result<_>>()?;
    ensure!(
        samples.len() == 9,
        "expected exactly nine save samples, got {}",
        samples.len()
    );
    let mut seen = [false; 9];
    let mut saves = Vec::new();
    let mut writes = Vec::new();
    for sample in samples {
        ensure!(sample.status == "ok", "failed save sample");
        ensure!(
            sample.scenario == "large_dirty_buffer_save",
            "unexpected probe scenario"
        );
        let Some(seen) = seen.get_mut(sample.step_index) else {
            bail!("unexpected save step {}", sample.step_index);
        };
        ensure!(!*seen, "duplicate save step {}", sample.step_index);
        *seen = true;
        let save = sample.phase_elapsed_ns.save_ns;
        ensure!(save.is_finite() && save > 0.0, "invalid save duration");
        saves.push(save / 1e6);
        if let Some(bytes) = sample.process_resources.write_bytes {
            writes.push(bytes);
        }
    }
    saves.sort_by(f64::total_cmp);
    writes.sort_unstable();
    let available = writes.len() == 9;
    Ok(Summary {
        launch,
        median_save_ms: saves[4],
        median_write_bytes: if available { Some(writes[4]) } else { None },
        write_bytes_available: available,
    })
}

#[cfg(test)]
mod tests {
    use super::summarize;
    #[cfg(unix)]
    use super::{measure, MeasureLargeDirtySaveArgs};
    use serde_json::{json, Value};
    #[cfg(unix)]
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    fn samples() -> Vec<Value> {
        (0..9)
            .map(|step| {
                json!({
                    "scenario": "large_dirty_buffer_save", "step_index": step, "status": "ok",
                    "phase_elapsed_ns": {"save_ns": (9 - step) * 1_000_000},
                    "process_resources": {"write_bytes": step * 100},
                })
            })
            .collect()
    }

    fn ndjson(samples: &[Value]) -> String {
        samples
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    #[test]
    fn medians_preserve_null_io_instead_of_inventing_zero() {
        let mut rows = samples();
        let summary = summarize(&ndjson(&rows), 2).unwrap();
        assert_eq!(summary.launch, 2);
        assert_eq!(summary.median_save_ms, 5.0);
        assert_eq!(summary.median_write_bytes, Some(400));
        assert!(summary.write_bytes_available);
        rows[0]["process_resources"]["write_bytes"] = Value::Null;
        let summary = summarize(&ndjson(&rows), 0).unwrap();
        assert_eq!(summary.median_write_bytes, None);
        assert!(!summary.write_bytes_available);
    }

    #[test]
    fn rejects_failed_missing_extra_duplicate_wrong_scenario_and_malformed_samples() {
        let mut rows = samples();
        rows.pop();
        assert!(summarize(&ndjson(&rows), 0).is_err());
        let mut rows = samples();
        rows.push(rows[0].clone());
        assert!(summarize(&ndjson(&rows), 0).is_err());
        for (field, value) in [
            ("status", json!("panic")),
            ("scenario", json!("unrelated")),
            ("step_index", json!(1)),
            ("step_index", json!(9)),
            ("phase_elapsed_ns", json!({"save_ns": -1})),
            ("phase_elapsed_ns", json!({"save_ns": null})),
            ("process_resources", json!({"write_bytes": -1})),
        ] {
            let mut rows = samples();
            rows[0][field] = value;
            assert!(summarize(&ndjson(&rows), 0).is_err(), "{field}");
        }
        assert!(summarize("", 0).is_err());
        assert!(summarize(&(ndjson(&samples()) + "{"), 0).is_err());
    }

    #[cfg(unix)]
    fn probe(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("probe");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn failures_and_timeouts_preserve_raw_evidence_without_success_summary() {
        for body in [
            "echo partial; echo failure >&2; exit 7",
            "echo partial; exec sleep 5",
            "echo '{invalid'",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let args = MeasureLargeDirtySaveArgs {
                binary: probe(dir.path(), body),
                output: dir.path().join("archive"),
                launches: 1,
            };
            let started = Instant::now();
            assert!(measure(&args, Duration::from_millis(100)).is_err());
            assert!(started.elapsed() < Duration::from_secs(4));
            assert!(args.output.join("metadata.json").is_file());
            assert!(!fs::read(args.output.join("run-0.jsonl"))
                .unwrap()
                .is_empty());
            assert!(!args.output.join("summary.json").exists());
        }
    }
}
