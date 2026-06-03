use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SCHEMA_ID: &str = "performance_review";
pub const SCHEMA_FILE: &str = "performance_review.schema.json";

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PerformanceReviewArtifact {
    pub meta: PerformanceReviewMeta,
    pub summary: PerformanceReviewSummary,
    pub target_promises: Vec<String>,
    pub promise_count: usize,
    pub promise_registry: Vec<PromiseRegistryEntry>,
    pub scenarios: Vec<PerformanceReviewScenario>,
    pub opportunities: Vec<String>,
    pub presentation: Presentation,
    pub coordinated_triage: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PerformanceReviewMeta {
    pub generated_from: String,
    pub source_artifacts: Vec<SourceArtifactStatus>,
    pub scenario_model: String,
    pub probe_classes: BTreeMap<String, ProbeClass>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct SourceArtifactStatus {
    pub id: String,
    pub path: String,
    pub available: bool,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ProbeClass {
    pub label: String,
    pub purpose: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PerformanceReviewSummary {
    pub scenario_count: usize,
    pub covered_scenarios: usize,
    pub thin_scenarios: usize,
    pub missing_scenarios: usize,
    pub coverage_gaps: usize,
    pub missing_scale_targets: usize,
    pub budget_misses: usize,
    pub ceilings_reached: usize,
    pub implementation_count: usize,
    pub ceiling_health_probes: usize,
    pub targeted_path_probes: usize,
    pub latency_rows: usize,
    pub capacity_rows: usize,
    pub resource_rows: usize,
    pub flamegraph_rows: usize,
    pub promise_health: PromiseHealthSummary,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PromiseHealthSummary {
    pub pass: usize,
    pub at_risk: usize,
    pub fail: usize,
    pub unmeasured: usize,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PromiseRegistryEntry {
    pub scenario_id: String,
    pub title: String,
    pub promise: String,
    pub scale_targets: Vec<PromisedScale>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PerformanceReviewScenario {
    pub id: String,
    pub title: String,
    pub promise: String,
    pub promise_health: PromiseHealth,
    pub synthetic_evidence: bool,
    pub coverage_status: CoverageStatus,
    pub coverage_score: f64,
    pub axes: CoverageAxes,
    pub promised_scale: Vec<PromisedScale>,
    pub scale_checks: Vec<ScaleCheck>,
    pub gaps: Vec<String>,
    pub opportunities: Vec<String>,
    pub implementations: Vec<Value>,
    pub implementation_count: usize,
    pub measurement_split: MeasurementSplit,
    pub budget_misses: usize,
    pub ceilings_reached: usize,
    pub scale_failures: usize,
    pub max_latency_ms: Option<f64>,
    pub peak_working_set_bytes: Option<i64>,
    pub evidence: ScenarioEvidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromiseHealth {
    Pass,
    AtRisk,
    Fail,
    Unmeasured,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Covered,
    Thin,
    Missing,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CoverageAxes {
    pub speed: CoverageAxis,
    pub capacity: CoverageAxis,
    pub resource: CoverageAxis,
    pub profiles: CoverageAxis,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct CoverageAxis {
    pub required: bool,
    pub covered: bool,
    pub count: usize,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct PromisedScale {
    pub id: String,
    pub label: String,
    pub minimum: i64,
    pub unit: String,
    pub capacity_scenarios: Vec<String>,
    pub resource_scenarios: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ScaleCheck {
    pub id: String,
    pub label: String,
    pub minimum: i64,
    pub unit: String,
    pub max_workload_value: Option<i64>,
    pub first_failure_workload: Option<i64>,
    pub last_successful_workload: Option<i64>,
    pub headroom_workload: Option<i64>,
    pub evidence_count: usize,
    pub synthetic: bool,
    pub met: bool,
    pub ceiling_status: CeilingStatus,
    pub status: ScaleCheckStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleCheckStatus {
    Met,
    Missing,
    Unmeasured,
    FailedBeforePromise,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CeilingStatus {
    NotReached,
    FailedAfterPromise,
    FailedBeforePromise,
    Unmeasured,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct MeasurementSplit {
    pub ceiling_health: MeasurementClassSummary,
    pub targeted_path: MeasurementClassSummary,
    pub diagnostic_profile: MeasurementClassSummary,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct MeasurementClassSummary {
    pub label: String,
    pub purpose: String,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ceilings_reached: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_misses: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct ScenarioEvidence {
    pub latency: Vec<Value>,
    pub capacity: Vec<Value>,
    pub resources: Vec<Value>,
    pub profiles: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
pub struct Presentation {
    pub notes: Vec<String>,
    pub primary_view: String,
    pub secondary_view: String,
}
