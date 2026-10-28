use crate::app_package;
use crate::artifacts;
use crate::config::LensConfig;
use crate::producers;
use anyhow::{bail, Result};
use clap::{builder::PossibleValuesParser, Parser, Subcommand};
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;

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
    Telemetry(ConfigArgs),
    Schema(SchemaArgs),
    /// Compare archived session latency without relaxing benchmark budgets.
    CompareSessionBaseline(crate::session_tools::CompareSessionBaselineArgs),
    /// Archive independent large-dirty-save probe launches (opt-in diagnostic).
    MeasureLargeDirtySave(crate::session_tools::MeasureLargeDirtySaveArgs),
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
}

fn measure_selection_values() -> PossibleValuesParser {
    PossibleValuesParser::new(
        ["all", "all-with-flamegraphs"]
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct MeasureOptions {
    pub skip_bench: bool,
    pub fail_on_slow: bool,
    pub index_only: bool,
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Catalog(args) => catalog(args),
        Command::Telemetry(args) => telemetry(args),
        Command::Measure(args) => measure(args),
        Command::Schema(args) => schema(args),
        Command::CompareSessionBaseline(args) => {
            crate::session_tools::compare_session_baseline(args)
        }
        Command::MeasureLargeDirtySave(args) => {
            crate::session_tools::measure_large_dirty_save(args)
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
    };
    let selected = selected_producers(&args.tool);
    validate_measure_options(&selected, options)?;

    let mut failures = Vec::new();
    let mut completed_benchmarks = HashSet::new();
    for producer in selected {
        let mut producer_options = options;
        if !producer.benchmarks.is_empty()
            && producer
                .benchmarks
                .iter()
                .all(|benchmark| completed_benchmarks.contains(benchmark))
        {
            producer_options.skip_bench = true;
        }
        let started = crate::provenance::timestamp();
        let context = crate::provenance::context(&config.project_root);
        let result = (producer.run)(&config, producer_options);
        if config.output_dir.join(producer.artifact).exists() {
            crate::provenance::record(
                &config,
                producer.artifact,
                started,
                &context,
                result.is_ok(),
            )?;
        }
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
    if !failures.is_empty() {
        bail!(
            "{} measure producer(s) failed:\n- {}",
            failures.len(),
            failures.join("\n- ")
        );
    }
    Ok(())
}

fn selected_producers(selection: &str) -> Vec<&'static ProducerSpec> {
    match selection {
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

fn validate_measure_options(selected: &[&ProducerSpec], options: MeasureOptions) -> Result<()> {
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
