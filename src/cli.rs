use crate::app_package;
use crate::artifacts::performance_review::{
    PerformanceReviewArtifact, SCHEMA_FILE as PERFORMANCE_REVIEW_SCHEMA_FILE,
    SCHEMA_ID as PERFORMANCE_REVIEW_SCHEMA_ID,
};
use crate::config::LensConfig;
use crate::producers;
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::path::PathBuf;

const PRODUCERS: &[ProducerSpec] = &[
    ProducerSpec::standard("slowspots", "slowspots.json", MeasureTool::Slowspots),
    ProducerSpec::standard(
        "frame-metrics",
        "frame_metrics.json",
        MeasureTool::FrameMetrics,
    ),
    ProducerSpec::standard("search", "search_speed.json", MeasureTool::Search),
    ProducerSpec::standard("capacity", "capacity_report.json", MeasureTool::Capacity),
    ProducerSpec::standard(
        "resources",
        "resource_profiles.json",
        MeasureTool::Resources,
    ),
    ProducerSpec::extra("flamegraphs", "flamegraphs.json", MeasureTool::Flamegraphs),
    ProducerSpec::standard(
        "speed-report",
        "speed_efficiency_report.json",
        MeasureTool::SpeedReport,
    ),
    ProducerSpec::standard(
        "performance-review",
        "performance_review.json",
        MeasureTool::PerformanceReview,
    ),
    ProducerSpec::standard(
        "project-code",
        "project_code_metrics.json",
        MeasureTool::ProjectCode,
    ),
];

#[derive(Clone, Copy, Debug)]
struct ProducerSpec {
    tool: &'static str,
    artifact: &'static str,
    measure_tool: MeasureTool,
    standard_run: bool,
}

impl ProducerSpec {
    const fn standard(
        tool: &'static str,
        artifact: &'static str,
        measure_tool: MeasureTool,
    ) -> Self {
        Self {
            tool,
            artifact,
            measure_tool,
            standard_run: true,
        }
    }

    const fn extra(tool: &'static str, artifact: &'static str, measure_tool: MeasureTool) -> Self {
        Self {
            tool,
            artifact,
            measure_tool,
            standard_run: false,
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
}

#[derive(Debug, Parser)]
struct ConfigArgs {
    #[arg(long)]
    config: Option<PathBuf>,
}

#[derive(Debug, Parser)]
struct MeasureArgs {
    #[arg(default_value = "all")]
    tool: MeasureSelection,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    skip_bench: bool,
    #[arg(long)]
    fail_on_slow: bool,
    #[arg(long)]
    index_only: bool,
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

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MeasureSelection {
    All,
    AllWithFlamegraphs,
    Slowspots,
    #[value(name = "frame-metrics")]
    FrameMetrics,
    Search,
    Capacity,
    Resources,
    Flamegraphs,
    #[value(name = "speed-report")]
    SpeedReport,
    #[value(name = "performance-review")]
    PerformanceReview,
    #[value(name = "project-code")]
    ProjectCode,
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
        SchemaCommand::Export(args) => export_schemas(args),
    }
}

fn export_schemas(args: SchemaExportArgs) -> Result<()> {
    std::fs::create_dir_all(&args.output)?;
    let performance_review_schema = schemars::schema_for!(PerformanceReviewArtifact);
    let performance_review_path = args.output.join(PERFORMANCE_REVIEW_SCHEMA_FILE);
    std::fs::write(
        &performance_review_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&performance_review_schema)?
        ),
    )?;
    let index = json!({
        "version": 1,
        "schemas": [{
            "id": PERFORMANCE_REVIEW_SCHEMA_ID,
            "file": PERFORMANCE_REVIEW_SCHEMA_FILE,
            "artifact": "performance_review.json",
        }],
    });
    std::fs::write(
        args.output.join("index.json"),
        format!("{}\n", serde_json::to_string_pretty(&index)?),
    )?;
    println!("Wrote schemas to {}", args.output.display());
    Ok(())
}

fn measure(args: MeasureArgs) -> Result<()> {
    let config = LensConfig::load(args.config.as_deref())?;
    std::fs::create_dir_all(&config.output_dir)?;
    let options = MeasureOptions {
        skip_bench: args.skip_bench,
        fail_on_slow: args.fail_on_slow,
        index_only: args.index_only,
    };
    for tool in selected_tools(args.tool) {
        run_measure_tool(tool, &config, options)?;
    }
    Ok(())
}

fn selected_tools(selection: MeasureSelection) -> Vec<MeasureTool> {
    match selection {
        MeasureSelection::All => standard_run_order(),
        MeasureSelection::AllWithFlamegraphs => {
            let mut tools = standard_run_order();
            tools.push(MeasureTool::Flamegraphs);
            tools
        }
        MeasureSelection::Slowspots => vec![MeasureTool::Slowspots],
        MeasureSelection::FrameMetrics => vec![MeasureTool::FrameMetrics],
        MeasureSelection::Search => vec![MeasureTool::Search],
        MeasureSelection::Capacity => vec![MeasureTool::Capacity],
        MeasureSelection::Resources => vec![MeasureTool::Resources],
        MeasureSelection::Flamegraphs => vec![MeasureTool::Flamegraphs],
        MeasureSelection::SpeedReport => vec![MeasureTool::SpeedReport],
        MeasureSelection::PerformanceReview => vec![MeasureTool::PerformanceReview],
        MeasureSelection::ProjectCode => vec![MeasureTool::ProjectCode],
    }
}

fn standard_run_order() -> Vec<MeasureTool> {
    PRODUCERS
        .iter()
        .filter(|producer| producer.standard_run)
        .map(|producer| producer.measure_tool)
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum MeasureTool {
    Slowspots,
    FrameMetrics,
    Search,
    Capacity,
    Resources,
    Flamegraphs,
    SpeedReport,
    PerformanceReview,
    ProjectCode,
}

fn run_measure_tool(tool: MeasureTool, config: &LensConfig, options: MeasureOptions) -> Result<()> {
    match tool {
        MeasureTool::Slowspots => producers::slowspots(config, options),
        MeasureTool::FrameMetrics => producers::frame_metrics(config),
        MeasureTool::Search => producers::search_speed(config, options),
        MeasureTool::Capacity => producers::capacity_report(config),
        MeasureTool::Resources => producers::resource_profiles(config),
        MeasureTool::Flamegraphs => producers::flamegraphs(config, options),
        MeasureTool::SpeedReport => producers::speed_efficiency_report(config),
        MeasureTool::PerformanceReview => producers::performance_review(config),
        MeasureTool::ProjectCode => producers::project_code_metrics(config),
    }
}

fn title_case(tool: &str) -> String {
    tool.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
