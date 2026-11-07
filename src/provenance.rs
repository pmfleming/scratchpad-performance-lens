//! Content-bound provenance. A successful process alone is not fresh evidence.
use crate::{config::LensConfig, shared};
use anyhow::Result;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// Deterministic content fingerprint (not a security/authenticity signature).
pub fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn command(root: &Path, program: &str, args: &[&str]) -> Option<String> {
    crate::process::text(Command::new(program).args(args).current_dir(root))
}

fn executable_fingerprint() -> Option<&'static str> {
    static HASH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| std::fs::read(path).ok())
            .map(|bytes| fingerprint(&bytes))
    })
    .as_deref()
}

struct RunSnapshot {
    root: PathBuf,
    value: Option<Value>,
    conditions: Value,
    control: Value,
}

thread_local! {
    // Producers execute synchronously. A scoped cache avoids process-global stale
    // contexts and keeps concurrent tests/projects isolated.
    static RUN_SNAPSHOT: RefCell<Option<RunSnapshot>> = const { RefCell::new(None) };
}

pub(crate) fn with_run_snapshot<T>(root: &Path, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<RunSnapshot>);
    impl Drop for Restore {
        fn drop(&mut self) {
            RUN_SNAPSHOT.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(RUN_SNAPSHOT.with(|slot| {
        slot.replace(Some(RunSnapshot {
            root: root.to_owned(),
            value: None,
            conditions: crate::environment::conditions(),
            control: crate::environment::control(),
        }))
    }));
    run()
}

pub fn context(root: &Path) -> Value {
    RUN_SNAPSHOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        match slot.as_mut().filter(|snapshot| snapshot.root == root) {
            Some(snapshot) => snapshot
                .value
                .get_or_insert_with(|| compute_context(root))
                .clone(),
            None => compute_context(root),
        }
    })
}

/// Bypass the per-run snapshot when checking for edits during a diagnostic run.
pub(crate) fn compute_context(root: &Path) -> Value {
    let files = Command::new("git")
        .args(["ls-files", "-c", "-o", "--exclude-standard", "-z"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|out| out.status.success());
    let mut contents = Vec::new();
    if let Some(files) = files {
        let mut paths: Vec<_> = files
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .collect();
        paths.sort();
        paths.dedup();
        for path in paths {
            contents.extend_from_slice(path);
            contents.push(0);
            let path = root.join(String::from_utf8_lossy(path).as_ref());
            match std::fs::read(path) {
                Ok(bytes) => contents.extend_from_slice(fingerprint(&bytes).as_bytes()),
                Err(_) => contents.extend_from_slice(b"missing"),
            }
        }
    }
    json!({
        "measurement_environment": crate::environment::identity(root),
        "project_root": root.canonicalize().ok(),
        "revision": command(root, "git", &["rev-parse", "HEAD"]),
        "dirty_state": command(root, "git", &["status", "--porcelain"]),
        "source_fingerprint": fingerprint(&contents),
        "toolchain": command(root, "rustc", &["-vV"]),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "host": command(root, "hostname", &[]),
        "cpu": std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|text|
            text.lines().find(|line| line.starts_with("model name")).map(str::to_owned)),
        "logical_cpus": std::thread::available_parallelism().ok().map(|value| value.get()),
        "rustflags": std::env::var("RUSTFLAGS").ok(),
        "cargo_target_dir": std::env::var("CARGO_TARGET_DIR").ok(),
        "lens_version": env!("CARGO_PKG_VERSION"),
        "lens_executable_fingerprint": executable_fingerprint(),
        "probe_environment": std::env::vars().filter(|(key, _)| key.starts_with("SCRATCHPAD_")
            || matches!(key.as_str(), "CARGO_BUILD_TARGET" | "CARGO_ENCODED_RUSTFLAGS" | "RUSTUP_TOOLCHAIN"))
            .collect::<std::collections::BTreeMap<_, _>>(),
    })
}

pub fn sidecar(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.provenance.json",
        path.file_name().unwrap_or_default().to_string_lossy()
    ))
}

pub fn record(
    config: &LensConfig,
    file: &str,
    started: f64,
    context: &Value,
    success: bool,
) -> Result<()> {
    let path = config.output_dir.join(file);
    let bytes = std::fs::read(&path)?;
    let artifact: Value = serde_json::from_slice(&bytes)?;
    // Budget failures are valid measurements. A structurally partial probe also
    // retains its real successful/failed samples; per-row completeness gates proof.
    let measured_failure = artifact.as_array().is_some_and(|rows| {
        !rows.is_empty() && rows.iter().all(|row| row["measurement_status"] == "valid")
    }) || artifact["meta"]["probe_status"] == "partial";
    let (before_conditions, before_control) = RUN_SNAPSHOT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|s| (s.conditions.clone(), s.control.clone()))
            .unwrap_or((Value::Null, Value::Null))
    });
    let after_control = crate::environment::control();
    let payload = json!({"version": 1,
        "conditions":{"before":before_conditions,"after":crate::environment::conditions()},
        "measurement_quality":crate::environment::quality(&before_control,&after_control), "run_id": format!("{}-{started:.9}", std::process::id()),
        "started_at": started, "finished_at": timestamp(), "context": context,
        "producer_success": success, "artifact_fingerprint": fingerprint(&bytes),
        "status": if success || measured_failure { "valid" } else { "failed" }});
    std::fs::write(sidecar(&path), serde_json::to_vec_pretty(&payload)?)?;
    Ok(())
}

pub fn validity(path: &Path, current_context: &Value) -> &'static str {
    let shared::JsonRead::Loaded(meta) = shared::read_json(&sidecar(path)) else {
        return "unverified";
    };
    if meta["status"] != "valid" {
        return "failed";
    }
    let Ok(bytes) = std::fs::read(path) else {
        return "missing";
    };
    if meta["artifact_fingerprint"] != fingerprint(&bytes) {
        return "invalid";
    }
    if &meta["context"] != current_context {
        return "stale";
    }
    // Same-revision measurements still expire: scheduling/thermal/environment conditions change.
    if meta["finished_at"]
        .as_f64()
        .is_none_or(|time| !(0.0..=86400.0).contains(&(timestamp() - time)))
    {
        return "stale";
    }
    "valid"
}

#[cfg(test)]
mod tests {
    use super::{record, timestamp, validity};
    use crate::config::LensConfig;
    use serde_json::json;
    #[test]
    fn snapshots_are_lazy_scoped_and_project_specific() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let other = tempfile::tempdir()?;
        assert!(crate::shared::run_command(root.path(), &["git", "init"])?
            .status
            .success());
        let path = root.path().join("Cargo.lock");
        let before = super::with_run_snapshot(root.path(), || -> anyhow::Result<_> {
            super::RUN_SNAPSHOT
                .with(|slot| assert!(slot.borrow().as_ref().unwrap().value.is_none()));
            std::fs::write(&path, "created by build")?;
            let captured = super::context(root.path());
            assert_eq!(captured, super::context(root.path()));
            std::fs::write(&path, "changed after snapshot")?;
            assert_eq!(captured, super::context(root.path()));
            assert_ne!(captured, super::context(other.path()));
            super::with_run_snapshot(other.path(), || {
                assert_ne!(captured, super::context(other.path()))
            });
            assert_eq!(captured, super::context(root.path()));
            Ok(captured)
        })?;
        assert_ne!(before, super::context(root.path()));
        super::RUN_SNAPSHOT.with(|slot| assert!(slot.borrow().is_none()));
        Ok(())
    }

    #[test]
    fn validity_requires_success_matching_context_and_unchanged_content() {
        let dir = tempfile::tempdir().unwrap();
        let config = LensConfig {
            project_name: "test".into(),
            project_root: dir.path().into(),
            output_dir: dir.path().into(),
        };
        let path = dir.path().join("test.json");
        std::fs::write(&path, "[]").unwrap();
        let context = json!({"revision": "a"});
        assert_eq!(validity(&path, &context), "unverified");
        record(&config, "test.json", timestamp(), &context, true).unwrap();
        assert_eq!(validity(&path, &context), "valid");
        assert_eq!(validity(&path, &json!({"revision":"b"})), "stale");
        std::fs::write(&path, "[1]").unwrap();
        assert_eq!(validity(&path, &context), "invalid");
    }
}
