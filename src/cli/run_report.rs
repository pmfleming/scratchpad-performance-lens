//! Runner cost is diagnostic metadata, never application latency or promise proof.
use super::{MeasureOptions, ProducerSpec};
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) struct RunReport {
    path: PathBuf,
    started: Instant,
    payload: Value,
}

impl RunReport {
    pub(super) fn start(
        output: &Path,
        selection: &str,
        producers: &[&ProducerSpec],
    ) -> Result<Self> {
        let report = Self {
            path: output.join("measure_run.json"),
            started: Instant::now(),
            payload: json!({
                "schema_version": 1,
                "selection": selection,
                "started_at": crate::provenance::timestamp(),
                "status": "running",
                "measurement_scope": "lens_wall_time_including_build_setup_probes_and_reporting",
                "promise_evidence": false,
                "producers": producers.iter().map(|p| json!({
                    "tool": p.tool, "artifact": p.artifact, "status": "pending",
                    "elapsed_seconds": null, "error": null,
                })).collect::<Vec<_>>(),
            }),
        };
        report.save()?;
        Ok(report)
    }

    pub(super) fn begin(&mut self, producer: &ProducerSpec, options: MeasureOptions) -> Result<()> {
        let row = self.row(producer);
        row["status"] = json!("running");
        row["started_at"] = json!(crate::provenance::timestamp());
        row["criterion_cache_requested"] =
            json!(!producer.benchmarks.is_empty() && options.skip_bench);
        row["requested_scenarios"] = json!(options.scenarios);
        self.save()
    }

    pub(super) fn finish(
        &mut self,
        producer: &ProducerSpec,
        elapsed: Duration,
        result: &Result<()>,
    ) -> Result<()> {
        let row = self.row(producer);
        row["status"] = json!(if result.is_ok() {
            "completed"
        } else {
            "failed"
        });
        row["elapsed_seconds"] = json!(elapsed.as_secs_f64());
        row["error"] = json!(result.as_ref().err().map(|e| format!("{e:#}")));
        self.save()
    }

    pub(super) fn complete(&mut self, success: bool) -> Result<()> {
        self.payload["status"] = json!(if success { "completed" } else { "failed" });
        self.payload["elapsed_seconds"] = json!(self.started.elapsed().as_secs_f64());
        self.save()
    }

    pub(super) fn abort(&mut self, error: &anyhow::Error) -> Result<()> {
        self.payload["error"] = json!(format!("{error:#}"));
        self.complete(false)
    }

    fn row(&mut self, producer: &ProducerSpec) -> &mut Value {
        self.payload["producers"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["tool"] == producer.tool)
            .unwrap()
    }

    fn save(&self) -> Result<()> {
        // Persist between producers so a stopped run retains completed costs and
        // the active operation, rather than looking like a successful old run.
        std::fs::write(&self.path, serde_json::to_vec_pretty(&self.payload)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{MeasureOptions, ProducerSpec, RunReport};
    use anyhow::Result;
    use serde_json::Value;
    use std::time::Duration;

    #[test]
    fn failed_and_interrupted_runs_never_look_completed() -> Result<()> {
        let root = tempfile::tempdir()?;
        let first = ProducerSpec::new("first", "first.json", |_, _| Ok(()), &["suite"]);
        let second = ProducerSpec::new("second", "second.json", |_, _| Ok(()), &[]);
        let read = || -> Value {
            serde_json::from_slice(&std::fs::read(root.path().join("measure_run.json")).unwrap())
                .unwrap()
        };
        let mut report = RunReport::start(root.path(), "all", &[&first, &second])?;
        let options = MeasureOptions {
            skip_bench: true,
            fail_on_slow: false,
            index_only: false,
            scenarios: Vec::new(),
        };
        report.begin(&first, options.clone())?;
        report.finish(
            &first,
            Duration::from_secs(2),
            &Err(anyhow::anyhow!("probe failed")),
        )?;
        report.begin(&second, options)?;
        let partial = read();
        assert_eq!(partial["status"], "running");
        assert_eq!(partial["producers"][0]["error"], "probe failed");
        assert_eq!(partial["producers"][0]["elapsed_seconds"], 2.0);
        assert_eq!(partial["producers"][0]["criterion_cache_requested"], true);
        assert_eq!(partial["producers"][1]["status"], "running");
        report.finish(&second, Duration::from_secs(1), &Ok(()))?;
        report.complete(false)?;
        assert_eq!(read()["status"], "failed");
        assert_eq!(read()["promise_evidence"], false);
        // A new invocation cannot inherit the old durations or failure.
        RunReport::start(root.path(), "second", &[&second])?;
        assert_eq!(read()["producers"][0]["elapsed_seconds"], Value::Null);
        Ok(())
    }
}
