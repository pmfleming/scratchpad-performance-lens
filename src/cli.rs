use crate::app_package;
use crate::artifacts;
use crate::config::LensConfig;
use crate::producers;
use anyhow::{bail, Result};
use clap::{builder::PossibleValuesParser, Parser, Subcommand};
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;

mod run_report;

type ProducerRun = fn(&LensConfig, MeasureOptions) -> Result<()>;

const PRODUCERS: &[ProducerSpec] = &[
    ProducerSpec::new(
        "slowspots",
        "slowspots.json",
        producers::slowspots,
        &["search_speed", "frame_budget", "promise_latency"],
    ),
    ProducerSpec::new(
        "frame-metrics",
        "frame_metrics.json",
        producers::frame_metrics,
        &[],
    ),
    ProducerSpec::new(
        "search",
        "search_speed.json",
        producers::search_speed,
        &["search_speed"],
    ),
    ProducerSpec::new(
        "search-workflows",
        "search_workflow_metrics.json",
        producers::search_workflows,
        &[],
    ),
    ProducerSpec::new(
        "capacity",
        "capacity_report.json",
        producers::capacity_report,
        &[],
    ),
    ProducerSpec::new(
        "resources",
        "resource_profiles.json",
        producers::resource_profiles,
        &[],
    ),
    ProducerSpec {
        standard_run: false,
        supports_index_only: true,
        ..ProducerSpec::new(
            "flamegraphs",
            "flamegraphs.json",
            producers::flamegraphs,
            &[],
        )
    },
    ProducerSpec::new(
        "speed-report",
        "speed_efficiency_report.json",
        producers::speed_efficiency_report,
        &[],
    ),
    ProducerSpec::new(
        "performance-review",
        "performance_review.json",
        producers::performance_review,
        &[],
    ),
    ProducerSpec::new(
        "project-code",
        "project_code_metrics.json",
        producers::project_code_metrics,
        &[],
    ),
];

static PROMISE_CAPACITY: ProducerSpec = ProducerSpec::new(
    "capacity",
    "capacity_report.json",
    producers::promise_capacity_report,
    &[],
);
static PROMISE_RESOURCES: ProducerSpec = ProducerSpec::new(
    "resources",
    "resource_profiles.json",
    producers::promise_resource_profiles,
    &[],
);

#[derive(Clone, Copy, Debug)]
struct ProducerSpec {
    tool: &'static str,
    artifact: &'static str,
    run: ProducerRun,
    benchmarks: &'static [&'static str],
    standard_run: bool,
    supports_index_only: bool,
}

impl ProducerSpec {
    const fn new(
        tool: &'static str,
        artifact: &'static str,
        run: ProducerRun,
        benchmarks: &'static [&'static str],
    ) -> Self {
        Self {
            tool,
            artifact,
            run,
            benchmarks,
            standard_run: true,
            supports_index_only: false,
        }
    }
}

#[derive(Debug, Parser)]
#[command(version, about = "Scratchpad performance measurement JSON producers")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Measure(MeasureArgs),
    Catalog(ConfigArgs),
    /// Check required measurements and project-owned budgets without running probes.
    Validate(ConfigArgs),
    Telemetry(ConfigArgs),
    Schema(SchemaArgs),
    /// Compare archived session latency without relaxing benchmark budgets.
    AcceptBaseline(crate::baselines::BaselineArgs),
    CompareBaseline(crate::baselines::BaselineArgs),
    CompareSessionBaseline(crate::session_tools::CompareSessionBaselineArgs),
    /// Archive independent large-dirty-save probe launches (opt-in diagnostic).
    MeasureLargeDirtySave(crate::session_tools::MeasureLargeDirtySaveArgs),
    /// Archive usage/archive/history maintenance and subsequent save samples.
    MeasureSessionMaintenance(crate::session_tools::MeasureLargeDirtySaveArgs),
}

#[derive(Debug, Parser)]
struct ConfigArgs {
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct MeasureArgs {
    #[arg(default_value = "all", value_parser = measure_selection_values())]
    tool: String,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, help = "Skip Criterion runs (slowspots and search only)")]
    skip_bench: bool,
    #[arg(
        long,
        help = "Fail on threshold violations (slowspots and search only)"
    )]
    fail_on_slow: bool,
    #[arg(long, help = "Only index existing SVGs (flamegraphs only)")]
    index_only: bool,
    #[arg(
        long = "scenario",
        help = "Run only this scenario; repeat for several (resources or capacity only)"
    )]
    scenarios: Vec<String>,
}

fn measure_selection_values() -> PossibleValuesParser {
    PossibleValuesParser::new(
        ["all", "promises", "all-with-flamegraphs"]
            .into_iter()
            .chain(PRODUCERS.iter().map(|producer| producer.tool)),
    )
}

#[derive(Debug, Parser)]
struct SchemaArgs {
    #[command(subcommand)]
    command: SchemaCommand,
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    Export(SchemaExportArgs),
}

#[derive(Debug, Parser)]
struct SchemaExportArgs {
    #[arg(long, default_value = "schemas")]
    output: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct MeasureOptions {
    pub skip_bench: bool,
    pub fail_on_slow: bool,
    pub index_only: bool,
    pub scenarios: Vec<String>,
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Catalog(args) => catalog(args),
        Command::Validate(args) => {
            let config = LensConfig::load(args.config.as_deref())?;
            producers::validate_registry(&config.project_root)?;
            println!("Promise registry is satisfiable.");
            Ok(())
        }
        Command::Telemetry(args) => telemetry(args),
        Command::Measure(args) => measure(args),
        Command::Schema(args) => schema(args),
        Command::AcceptBaseline(args) => crate::baselines::accept(args),
        Command::CompareBaseline(args) => crate::baselines::compare(args),
        Command::CompareSessionBaseline(args) => {
            crate::session_tools::compare_session_baseline(args)
        }
        Command::MeasureLargeDirtySave(args) => {
            crate::session_tools::measure_large_dirty_save(args)
        }
        Command::MeasureSessionMaintenance(args) => {
            crate::session_tools::measure_session_maintenance(args)
        }
    }
}

fn catalog(args: ConfigArgs) -> Result<()> {
    let config = LensConfig::load(args.config.as_deref())?;
    let tasks: Vec<_> = PRODUCERS
        .iter()
        .map(|producer| {
            json!({
                "id": format!("performance.{}", producer.tool),
                "category": "performance",
                "title": title_case(producer.tool),
                "output_artifacts": [config.output_dir.join(producer.artifact).to_string_lossy()],
            })
        })
        .chain(std::iter::once(json!({
            "id": "telemetry.app-package",
            "category": "telemetry",
            "title": "App Package",
            "output_artifacts": [],
        })))
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "version": 1,
            "project_name": config.project_name,
            "tasks": tasks,
        }))?
    );
    Ok(())
}

fn telemetry(args: ConfigArgs) -> Result<()> {
    let _config = LensConfig::load(args.config.as_deref())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&app_package::payload()?)?
    );
    Ok(())
}

fn schema(args: SchemaArgs) -> Result<()> {
    match args.command {
        SchemaCommand::Export(args) => artifacts::export_schemas(&args.output),
    }
}

fn measure(args: MeasureArgs) -> Result<()> {
    let config = LensConfig::load(args.config.as_deref())?;
    std::fs::create_dir_all(&config.output_dir)?;
    let options = MeasureOptions {
        skip_bench: args.skip_bench,
        fail_on_slow: args.fail_on_slow,
        index_only: args.index_only,
        scenarios: args.scenarios,
    };
    let selected = selected_producers(&args.tool);
    validate_measure_options(&selected, &options)?;
    let mut run_report = run_report::RunReport::start(&config.output_dir, &args.tool, &selected)?;

    if matches!(
        args.tool.as_str(),
        "all" | "promises" | "all-with-flamegraphs" | "performance-review"
    ) {
        if let Err(error) = producers::validate_registry(&config.project_root) {
            run_report.abort(&error)?;
            return Err(error);
        }
    }
    let mut failures = Vec::new();
    let mut completed_benchmarks = HashSet::new();
    for producer in selected {
        let mut producer_options = options.clone();
        if !producer.benchmarks.is_empty()
            && producer
                .benchmarks
                .iter()
                .all(|benchmark| completed_benchmarks.contains(benchmark))
        {
            producer_options.skip_bench = true;
        }
        run_report.begin(producer, producer_options.clone())?;
        eprintln!("Starting {}...", producer.tool);
        let start = std::time::Instant::now();
        let result = run_producer(&config, producer, producer_options);
        let elapsed = start.elapsed();
        run_report.finish(producer, elapsed, &result)?;
        eprintln!(
            "{} finished in {:.2}s ({})",
            producer.tool,
            elapsed.as_secs_f64(),
            if result.is_ok() {
                "completed"
            } else {
                "failed"
            }
        );
        match result {
            Ok(()) => {
                if !options.skip_bench {
                    completed_benchmarks.extend(producer.benchmarks.iter().copied());
                }
            }
            Err(error) => {
                eprintln!("{} failed: {error:#}", producer.tool);
                failures.push(format!("{}: {error:#}", producer.tool));
            }
        }
    }
    run_report.complete(failures.is_empty())?;
    if !failures.is_empty() {
        bail!(
            "{} measure producer(s) failed:\n- {}",
            failures.len(),
            failures.join("\n- ")
        );
    }
    Ok(())
}

fn run_producer(
    config: &LensConfig,
    producer: &ProducerSpec,
    options: MeasureOptions,
) -> Result<()> {
    // Remove both files before invocation: an early failure must never refresh old evidence.
    let path = config.output_dir.join(producer.artifact);
    for old in [&path, &crate::provenance::sidecar(&path)] {
        match std::fs::remove_file(old) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let started = crate::provenance::timestamp();
    crate::provenance::with_run_snapshot(&config.project_root, || {
        let result = (producer.run)(config, options);
        if path.exists() {
            // The snapshot is lazy: benchmark code first requests it after Cargo exits;
            // other probes first request it here. Reports reuse their read snapshot.
            let context = crate::provenance::context(&config.project_root);
            crate::provenance::record(
                config,
                producer.artifact,
                started,
                &context,
                result.is_ok(),
            )?;
        }
        result
    })
}

fn selected_producers(selection: &str) -> Vec<&'static ProducerSpec> {
    match selection {
        "promises" => PRODUCERS
            .iter()
            .filter(|producer| producer.standard_run && producer.tool != "project-code")
            .map(|producer| match producer.tool {
                "capacity" => &PROMISE_CAPACITY,
                "resources" => &PROMISE_RESOURCES,
                _ => producer,
            })
            .collect(),
        "all" => PRODUCERS
            .iter()
            .filter(|producer| producer.standard_run)
            .collect(),
        "all-with-flamegraphs" => PRODUCERS.iter().collect(),
        tool => PRODUCERS
            .iter()
            .find(|producer| producer.tool == tool)
            .into_iter()
            .collect(),
    }
}

fn validate_measure_options(selected: &[&ProducerSpec], options: &MeasureOptions) -> Result<()> {
    if !options.scenarios.is_empty()
        && (selected.len() != 1 || !matches!(selected[0].tool, "resources" | "capacity"))
    {
        bail!("--scenario only applies to a resources or capacity run; it cannot narrow a promise review");
    }
    if options.fail_on_slow
        && !selected
            .iter()
            .any(|producer| !producer.benchmarks.is_empty())
    {
        bail!("--fail-on-slow only applies to slowspots and search");
    }
    if options.index_only && !selected.iter().any(|producer| producer.supports_index_only) {
        bail!("--index-only only applies to flamegraphs");
    }
    Ok(())
}

fn title_case(tool: &str) -> String {
    tool.split('-')
        .map(crate::shared::capitalize)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{run_producer, LensConfig, MeasureOptions, ProducerSpec};
    use crate::provenance;
    use anyhow::{bail, Result};

    #[test]
    fn focused_probe_selection_cannot_narrow_promise_verification() {
        let options = MeasureOptions {
            skip_bench: false,
            fail_on_slow: false,
            index_only: false,
            scenarios: vec!["session_persist_cost".into()],
        };
        assert!(
            super::validate_measure_options(&super::selected_producers("resources"), &options)
                .is_ok()
        );
        assert!(
            super::validate_measure_options(&super::selected_producers("capacity"), &options)
                .is_ok()
        );
        for selection in ["promises", "all", "performance-review", "search"] {
            assert!(super::validate_measure_options(
                &super::selected_producers(selection),
                &options
            )
            .is_err());
        }
    }

    #[test]
    fn promise_run_keeps_measurements_and_reports_in_dependency_order() {
        let selected = super::selected_producers("promises");
        assert_eq!(
            selected.iter().map(|p| p.tool).collect::<Vec<_>>(),
            [
                "slowspots",
                "frame-metrics",
                "search",
                "search-workflows",
                "capacity",
                "resources",
                "speed-report",
                "performance-review"
            ]
        );
        assert!(std::ptr::eq(selected[4], &super::PROMISE_CAPACITY));
        assert!(std::ptr::eq(selected[5], &super::PROMISE_RESOURCES));
        assert_eq!(super::selected_producers("all").len(), 9);
    }

    #[test]
    fn early_failure_cannot_restamp_old_partial_and_new_output_binds_post_run_inputs() -> Result<()>
    {
        let root = tempfile::tempdir()?;
        assert!(crate::shared::run_command(root.path(), &["git", "init"])?
            .status
            .success());
        std::fs::write(root.path().join(".gitignore"), "target/\n")?;
        std::fs::write(root.path().join("Cargo.lock"), "before")?;
        let config = LensConfig {
            project_name: "fixture".into(),
            project_root: root.path().into(),
            output_dir: root.path().join("target/analysis"),
        };
        std::fs::create_dir_all(&config.output_dir)?;
        let path = config.output_dir.join("test.json");
        let old_context = provenance::context(root.path());
        std::fs::write(&path, r#"{"meta":{"probe_status":"partial"}}"#)?;
        provenance::record(&config, "test.json", 0.0, &old_context, false)?;
        let options = MeasureOptions {
            skip_bench: true,
            fail_on_slow: false,
            index_only: false,
            scenarios: Vec::new(),
        };
        let early = ProducerSpec::new(
            "test",
            "test.json",
            |_, _| bail!("typed decode failed"),
            &[],
        );
        assert!(run_producer(&config, &early, options.clone()).is_err());
        assert!(!path.exists());
        assert!(!provenance::sidecar(&path).exists());
        let fresh = ProducerSpec::new(
            "test",
            "test.json",
            |config, _| {
                std::fs::write(config.project_root.join("Cargo.lock"), "after")?;
                std::fs::write(
                    config.output_dir.join("test.json"),
                    r#"{"meta":{"probe_status":"partial"}}"#,
                )?;
                bail!("fresh partial evidence")
            },
            &[],
        );
        assert!(run_producer(&config, &fresh, options).is_err());
        assert_eq!(provenance::validity(&path, &old_context), "stale");
        assert_eq!(
            provenance::validity(&path, &provenance::context(root.path())),
            "valid"
        );
        Ok(())
    }
}
