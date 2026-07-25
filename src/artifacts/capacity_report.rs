use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_ID: &str = "capacity_report";
pub const SCHEMA_FILE: &str = "capacity_report.schema.json";

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct CapacityReportArtifact {
    pub meta: CapacityReportMeta,
    pub summary: CapacityReportSummary,
    pub scenarios: Vec<CapacityScenario>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct CapacityReportMeta {
    pub generated_from: String,
    pub probe_command: String,
    pub scenario_count: usize,
    pub probe_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub synthetic: bool,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct CapacityReportSummary {
    pub scenario_count: usize,
    pub ceilings_reached: usize,
    pub memory_bound_scenarios: usize,
    pub cpu_bound_scenarios: usize,
    pub probe_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub synthetic: bool,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct CapacityScenario {
    pub scenario: String,
    pub probe_class: String,
    pub measurement_role: String,
    pub measurement_question: String,
    pub scenario_label: String,
    pub workload_family: String,
    pub threshold_ms: f64,
    pub failure_mode: String,
    pub ceiling_reached: Option<bool>,
    pub last_successful_workload: Option<i64>,
    pub last_successful_label: Option<String>,
    pub first_failure_workload: Option<i64>,
    pub first_failure_label: Option<String>,
    pub peak_working_set_bytes: Option<i64>,
    pub working_set_growth_bytes: Option<i64>,
    pub page_fault_growth: Option<i64>,
    pub handle_growth: Option<i64>,
    pub first_saturated_resource: String,
    pub suspected_limiting_resource: String,
    pub matching_flamegraphs: Vec<String>,
    pub diagnosis_guidance: Vec<String>,
    pub resource_checklist: Vec<Value>,
    pub samples: Vec<CapacitySample>,
}

impl CapacityScenario {
    pub fn max_workload_value(&self) -> Option<i64> {
        self.last_successful_workload
            .into_iter()
            .chain(self.first_failure_workload)
            .chain(
                self.samples
                    .iter()
                    .filter_map(|sample| sample.workload_value),
            )
            .max()
    }
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema, Serialize)]
pub struct CapacitySample {
    pub workload_value: Option<i64>,
    pub workload_label: Option<String>,
    pub setup_elapsed_ms: Option<f64>,
    pub elapsed_ms: f64,
    pub background_completion_ms: Option<f64>,
    pub elapsed_min_ms: f64,
    pub elapsed_max_ms: f64,
    pub repetition_count: usize,
    pub measurement_scope: Option<String>,
    pub working_set_bytes: Option<i64>,
    pub page_fault_count: Option<i64>,
    pub handle_count: Option<i64>,
    pub status: String,
}
