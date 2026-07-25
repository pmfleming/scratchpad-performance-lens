mod evidence;
mod health;
mod registry;

use super::common::{array_value, read_analysis, scenarios_array};
use super::flamegraphs::fallback_flamegraphs;
use super::render::render_performance_review;
use crate::artifacts::capacity_report::{CapacityReportArtifact, CapacityScenario};
use crate::artifacts::performance_review::{
    CoverageAxes, CoverageStatus, MeasurementClassSummary, MeasurementSplit,
    PerformanceReviewArtifact, PerformanceReviewMeta, PerformanceReviewScenario,
    PerformanceReviewSummary, Presentation, PromiseHealth, PromiseHealthSummary,
    PromiseRegistryEntry, ScaleCheckStatus, ScenarioEvidence,
};
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::Result;
use evidence::{
    mean_ms, over_budget_latency, payload_synthetic, review_row_matches, source_status, unique_rows,
};
use health::{coverage_axis, promise_health, scale_checks, scenario_gaps, scenario_opportunities};
use registry::{probe_classes, promised_scale_payload, review_scenarios, ReviewScenario};
use serde_json::{json, Value};
pub fn performance_review(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let slowspots = read_analysis(config, "slowspots.json", json!([]));
    let search = read_analysis(config, "search_speed.json", json!([]));
    let capacity = serde_json::from_value::<CapacityReportArtifact>(read_analysis(
        config,
        "capacity_report.json",
        json!({"scenarios": []}),
    ))
    .unwrap_or_default();
    let resources = read_analysis(config, "resource_profiles.json", json!({"scenarios": []}));
    let flamegraphs = read_analysis(
        config,
        "flamegraphs.json",
        fallback_flamegraphs(&config.output_dir.join("flamegraphs")),
    );
    let speed_report = read_analysis(config, "speed_efficiency_report.json", json!({}));

    let latency_rows = unique_rows([array_value(&slowspots), array_value(&search)].concat());
    let capacity_rows = capacity.scenarios;
    let resource_rows = scenarios_array(&resources).to_vec();
    let capacity_synthetic = capacity.meta.synthetic;
    let resources_synthetic = payload_synthetic(&resources);
    let scenarios = review_scenarios();
    let scenario_payload: Vec<_> = scenarios
        .iter()
        .map(|scenario| {
            build_review_scenario(
                scenario,
                &latency_rows,
                &capacity_rows,
                &resource_rows,
                array_value(&flamegraphs),
                capacity_synthetic,
                resources_synthetic,
            )
        })
        .collect();
    let target_promises: Vec<_> = scenarios
        .iter()
        .map(|scenario| scenario.promise.to_string())
        .collect();
    let promise_registry: Vec<_> = scenarios
        .iter()
        .map(|scenario| PromiseRegistryEntry {
            scenario_id: scenario.id.to_string(),
            title: scenario.title.to_string(),
            promise: scenario.promise.to_string(),
            scale_targets: promised_scale_payload(scenario),
        })
        .collect();
    let covered = scenario_payload
        .iter()
        .filter(|row| row.coverage_status == CoverageStatus::Covered)
        .count();
    let thin = scenario_payload
        .iter()
        .filter(|row| row.coverage_status == CoverageStatus::Thin)
        .count();
    let missing = scenario_payload.len() - covered - thin;
    let artifact = PerformanceReviewArtifact {
        meta: PerformanceReviewMeta {
            generated_from: "rust:performance_review".to_string(),
            source_artifacts: source_status(config),
            scenario_model: "scenario-first performance coverage".to_string(),
            probe_classes: probe_classes(),
        },
        summary: PerformanceReviewSummary {
            scenario_count: scenario_payload.len(),
            covered_scenarios: covered,
            thin_scenarios: thin,
            missing_scenarios: missing,
            coverage_gaps: scenario_payload.iter().map(|row| row.gaps.len()).sum(),
            missing_scale_targets: scenario_payload
                .iter()
                .flat_map(|row| &row.scale_checks)
                .filter(|check| !check.met)
                .count(),
            budget_misses: scenario_payload.iter().map(|row| row.budget_misses).sum(),
            ceilings_reached: scenario_payload.iter().map(|row| row.ceilings_reached).sum(),
            implementation_count: scenario_payload
                .iter()
                .map(|row| row.implementation_count)
                .sum(),
            ceiling_health_probes: capacity_rows.len(),
            targeted_path_probes: latency_rows.len() + resource_rows.len(),
            latency_rows: latency_rows.len(),
            capacity_rows: capacity_rows.len(),
            resource_rows: resource_rows.len(),
            flamegraph_rows: array_value(&flamegraphs).len(),
            promise_health: PromiseHealthSummary {
                pass: scenario_payload
                    .iter()
                    .filter(|row| row.promise_health == PromiseHealth::Pass)
                    .count(),
                at_risk: scenario_payload
                    .iter()
                    .filter(|row| row.promise_health == PromiseHealth::AtRisk)
                    .count(),
                fail: scenario_payload
                    .iter()
                    .filter(|row| row.promise_health == PromiseHealth::Fail)
                    .count(),
                unmeasured: scenario_payload
                    .iter()
                    .filter(|row| row.promise_health == PromiseHealth::Unmeasured)
                    .count(),
            },
        },
        promise_count: target_promises.len(),
        target_promises,
        promise_registry,
        scenarios: scenario_payload,
        opportunities: Vec::new(),
        presentation: Presentation {
            notes: vec![
                "Lead with scenario coverage before raw dataset tables.".to_string(),
                "Keep ceiling probes and targeted path probes visually separate: ceilings prove promise health; targeted paths validate changes.".to_string(),
                "Keep latency, capacity, resources, and flamegraphs available as drill-down evidence.".to_string(),
            ],
            primary_view: "coverage_matrix".to_string(),
            secondary_view: "scenario_drilldown".to_string(),
        },
        coordinated_triage: speed_report
            .get("triage")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().take(5).cloned().collect::<Vec<_>>())
            .unwrap_or_default(),
    };
    let payload = serde_json::to_value(&artifact)?;
    shared::write_visibility(
        &config.output_dir.join("performance_review.json"),
        &payload,
        "performance review",
        render_performance_review(&payload),
    )
}
fn build_review_scenario(
    scenario: &ReviewScenario,
    latency: &[Value],
    capacity: &[CapacityScenario],
    resources: &[Value],
    flamegraphs: Vec<Value>,
    capacity_synthetic: bool,
    resources_synthetic: bool,
) -> PerformanceReviewScenario {
    let latency_rows: Vec<_> = latency
        .iter()
        .filter(|row| review_row_matches(row, scenario))
        .cloned()
        .collect();
    let capacity_rows: Vec<_> = capacity
        .iter()
        .filter(|row| scenario.capacity_scenarios.contains(&row.scenario.as_str()))
        .cloned()
        .collect();
    let resource_rows: Vec<_> = resources
        .iter()
        .filter(|row| {
            row.get("scenario")
                .and_then(Value::as_str)
                .is_some_and(|id| scenario.resource_scenarios.contains(&id))
        })
        .cloned()
        .collect();
    let profile_rows: Vec<_> = flamegraphs
        .into_iter()
        .filter(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| scenario.profile_ids.contains(&id))
        })
        .collect();
    let axes = CoverageAxes {
        speed: coverage_axis(latency_rows.len(), !scenario.benchmark_keys.is_empty()),
        capacity: coverage_axis(capacity_rows.len(), !scenario.capacity_scenarios.is_empty()),
        resource: coverage_axis(resource_rows.len(), !scenario.resource_scenarios.is_empty()),
        profiles: coverage_axis(profile_rows.len(), !scenario.profile_ids.is_empty()),
    };
    let axis_values = [&axes.speed, &axes.capacity, &axes.resource, &axes.profiles];
    let required = axis_values.iter().filter(|axis| axis.required).count();
    let covered = axis_values
        .iter()
        .filter(|axis| axis.required && axis.covered)
        .count();
    let coverage_status = if required == covered {
        CoverageStatus::Covered
    } else if covered > 0 {
        CoverageStatus::Thin
    } else {
        CoverageStatus::Missing
    };
    let gaps = scenario_gaps(&axes);
    let scale_checks = scale_checks(
        scenario,
        &capacity_rows,
        &resource_rows,
        capacity_synthetic,
        resources_synthetic,
    );
    let budget_misses = latency_rows.iter().filter(over_budget_latency).count();
    let ceilings_reached = capacity_rows
        .iter()
        .filter(|row| row.ceiling_reached.unwrap_or(false))
        .count();
    let scale_failures = scale_checks
        .iter()
        .filter(|check| check.status == ScaleCheckStatus::FailedBeforePromise)
        .count();
    let missing_scale_targets = scale_checks.iter().filter(|check| !check.met).count();
    let synthetic_evidence = (capacity_synthetic && !capacity_rows.is_empty())
        || (resources_synthetic && !resource_rows.is_empty());
    let promise_health = promise_health(
        coverage_status,
        budget_misses,
        scale_failures,
        missing_scale_targets,
        synthetic_evidence,
    );
    let gaps_empty = gaps.is_empty();
    PerformanceReviewScenario {
        id: scenario.id.to_string(),
        title: scenario.title.to_string(),
        promise: scenario.promise.to_string(),
        promise_health,
        synthetic_evidence,
        coverage_status,
        coverage_score: if required == 0 {
            1.0
        } else {
            (covered as f64 / required as f64 * 1000.0).round() / 1000.0
        },
        axes,
        promised_scale: promised_scale_payload(scenario),
        scale_checks,
        gaps,
        opportunities: scenario_opportunities(promise_health, gaps_empty),
        implementations: Vec::new(),
        implementation_count: 0,
        measurement_split: MeasurementSplit {
            ceiling_health: MeasurementClassSummary {
                label: "Ceiling / Promise Health".to_string(),
                purpose: "Shows whether a seven-promise scale boundary still passes.".to_string(),
                count: capacity_rows.len(),
                ceilings_reached: Some(ceilings_reached),
                budget_misses: None,
                available: None,
            },
            targeted_path: MeasurementClassSummary {
                label: "Targeted Path".to_string(),
                purpose:
                    "Shows whether a specific implementation path or recent change is working."
                        .to_string(),
                count: latency_rows.len() + resource_rows.len(),
                ceilings_reached: None,
                budget_misses: Some(budget_misses),
                available: None,
            },
            diagnostic_profile: MeasurementClassSummary {
                label: "Diagnostic Profile".to_string(),
                purpose:
                    "Explains where time goes after a targeted or ceiling probe finds pressure."
                        .to_string(),
                count: profile_rows.len(),
                ceilings_reached: None,
                budget_misses: None,
                available: Some(
                    profile_rows
                        .iter()
                        .filter(|row| {
                            row.get("available")
                                .and_then(Value::as_bool)
                                .unwrap_or(false)
                        })
                        .count(),
                ),
            },
        },
        budget_misses,
        ceilings_reached,
        scale_failures,
        max_latency_ms: latency_rows
            .iter()
            .filter_map(mean_ms)
            .fold(None, |max: Option<f64>, value| {
                Some(max.map_or(value, |old| old.max(value)))
            }),
        peak_working_set_bytes: capacity_rows
            .iter()
            .filter_map(|row| row.peak_working_set_bytes)
            .chain(resource_rows.iter().filter_map(|row| {
                row.get("peak_working_set_bytes")
                    .or_else(|| row.get("max_working_set_bytes"))
                    .and_then(Value::as_i64)
            }))
            .max(),
        evidence: ScenarioEvidence {
            latency: latency_rows,
            capacity: capacity_rows
                .into_iter()
                .filter_map(|row| serde_json::to_value(row).ok())
                .collect(),
            resources: resource_rows,
            profiles: profile_rows,
        },
    }
}
#[cfg(test)]
mod tests {
    use super::{build_review_scenario, review_scenarios};
    use crate::artifacts::capacity_report::{CapacitySample, CapacityScenario};
    use crate::artifacts::performance_review::{
        CeilingStatus, CoverageStatus, PromiseHealth, ScaleCheckStatus,
    };
    use crate::shared;
    use serde_json::{json, Value};

    fn capacity(mut value: Value) -> CapacityScenario {
        if let Some(samples) = value.get_mut("samples").and_then(Value::as_array_mut) {
            for sample in samples {
                let mut complete = serde_json::to_value(CapacitySample::default()).unwrap();
                complete
                    .as_object_mut()
                    .unwrap()
                    .extend(sample.as_object().unwrap().clone());
                *sample = complete;
            }
        }
        let mut complete = serde_json::to_value(CapacityScenario::default()).unwrap();
        complete
            .as_object_mut()
            .unwrap()
            .extend(value.as_object().unwrap().clone());
        serde_json::from_value(complete).unwrap()
    }

    #[test]
    fn promise_health_fails_even_when_coverage_is_complete() {
        let scenario = review_scenarios()
            .into_iter()
            .find(|scenario| scenario.id == "large_files")
            .unwrap();
        let row = build_review_scenario(
            &scenario,
            &[json!({
                "benchmark_key": "file_load",
                "workload_family": "file-load",
                "threshold_ms": 100.0,
                "budget_probe_ns": 125_000_000.0,
            })],
            &[capacity(json!({
                "scenario": "large_file_background_index_ceiling",
                "ceiling_reached": false,
                "samples": [{"workload_value": shared::GB}],
            }))],
            &[json!({
                "scenario": "large_utf8_load_peak_memory",
                "samples": [{"workload_value": shared::GB}],
            })],
            vec![json!({"id": "scroll_stress_profile", "available": true})],
            false,
            false,
        );
        assert_eq!(row.coverage_status, CoverageStatus::Covered);
        assert_eq!(row.promise_health, PromiseHealth::Fail);
        assert_eq!(row.budget_misses, 1);
    }

    #[test]
    fn synthetic_scale_evidence_is_unmeasured() {
        let scenario = review_scenarios()
            .into_iter()
            .find(|scenario| scenario.id == "many_tabs")
            .unwrap();
        let row = build_review_scenario(
            &scenario,
            &[],
            &[capacity(json!({
                "scenario": "tab_count_ceiling",
                "ceiling_reached": null,
                "samples": [{"workload_value": 10_000}],
            }))],
            &[],
            vec![],
            true,
            false,
        );
        assert_eq!(row.promise_health, PromiseHealth::Unmeasured);
        assert_eq!(row.scale_checks[0].status, ScaleCheckStatus::Unmeasured);
        assert!(!row.scale_checks[0].met);
    }

    #[test]
    fn ceiling_hit_after_promised_scale_still_passes() {
        let scenario = review_scenarios()
            .into_iter()
            .find(|scenario| scenario.id == "many_files")
            .unwrap();
        let row = build_review_scenario(
            &scenario,
            &[json!({
                "benchmark_key": "search_current_completion_aggregate_size",
                "workload_family": "search",
                "threshold_ms": 100.0,
                "budget_probe_ns": 80_000_000.0,
            })],
            &[capacity(json!({
                "scenario": "many_file_first_visible_ceiling",
                "ceiling_reached": true,
                "first_failure_workload": 50_000,
                "last_successful_workload": 10_000,
            }))],
            &[json!({
                "scenario": "many_file_resource_tracking",
                "samples": [{"workload_value": 10_000}],
            })],
            vec![json!({"id": "search_all_tabs_profile", "available": true})],
            false,
            false,
        );
        assert_eq!(row.coverage_status, CoverageStatus::Covered);
        assert_eq!(row.promise_health, PromiseHealth::Pass);
        assert_eq!(row.ceilings_reached, 1);
        assert_eq!(row.scale_failures, 0);
        assert!(row.scale_checks[0].met);
        assert_eq!(
            row.scale_checks[0].ceiling_status,
            CeilingStatus::FailedAfterPromise
        );
        assert_eq!(row.scale_checks[0].headroom_workload, Some(40_000));
    }

    #[test]
    fn ceiling_hit_before_promised_scale_fails_even_with_other_scale_evidence() {
        let scenario = review_scenarios()
            .into_iter()
            .find(|scenario| scenario.id == "many_files")
            .unwrap();
        let row = build_review_scenario(
            &scenario,
            &[json!({
                "benchmark_key": "search_current_completion_aggregate_size",
                "workload_family": "search",
                "threshold_ms": 100.0,
                "budget_probe_ns": 80_000_000.0,
            })],
            &[
                capacity(json!({
                    "scenario": "many_file_first_visible_ceiling",
                    "ceiling_reached": true,
                    "first_failure_workload": 5_000,
                    "last_successful_workload": 1_000,
                })),
                capacity(json!({
                    "scenario": "search_target_count_ceiling",
                    "ceiling_reached": false,
                    "last_successful_workload": 10_000,
                    "samples": [{"workload_value": 10_000}],
                })),
            ],
            &[json!({
                "scenario": "many_file_resource_tracking",
                "samples": [{"workload_value": 10_000}],
            })],
            vec![json!({"id": "search_all_tabs_profile", "available": true})],
            false,
            false,
        );
        assert_eq!(row.coverage_status, CoverageStatus::Covered);
        assert_eq!(row.promise_health, PromiseHealth::Fail);
        assert_eq!(row.scale_failures, 1);
        assert!(!row.scale_checks[0].met);
        assert_eq!(
            row.scale_checks[0].ceiling_status,
            CeilingStatus::FailedBeforePromise
        );
        assert_eq!(
            row.scale_checks[0].failed_capacity_scenarios,
            ["many_file_first_visible_ceiling"]
        );
    }
}
