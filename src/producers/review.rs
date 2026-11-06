mod coverage;
mod evidence;
mod health;
mod registry;
#[cfg(test)]
mod scale_tests;

use super::common::{scenarios_array, valid_frame, values_array, AnalysisInputs};
use super::flamegraphs::fallback_flamegraphs;
use super::render::render_performance_review;
use crate::artifacts::capacity_report::{CapacityReportArtifact, CapacityScenario};
use crate::artifacts::performance_review::{
    CoverageStatus, MeasurementClassSummary, MeasurementSplit, PerformanceReviewArtifact,
    PerformanceReviewMeta, PerformanceReviewScenario, PerformanceReviewSummary, Presentation,
    PromiseHealth, PromiseHealthSummary, PromiseRegistryEntry, ScaleCheckStatus, ScenarioEvidence,
};
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{Context, Result};
use evidence::{
    mean_ms, over_budget_latency, payload_synthetic, resource_contract_failed, review_row_matches,
    source_status, unique_rows,
};
use health::{promise_health, scale_checks, scenario_gaps, scenario_opportunities};
pub use registry::validate as validate_registry;
use registry::{probe_classes, promised_scale_payload, review_scenarios, ReviewScenario};
use serde_json::{json, Value};

pub fn performance_review(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    let inputs = AnalysisInputs::new(config);
    let slowspots = inputs.read("slowspots.json", json!([]));
    let search = inputs.read("search_speed.json", json!([]));
    let capacity = serde_json::from_value::<CapacityReportArtifact>(inputs.read(
        "capacity_report.json",
        serde_json::to_value(CapacityReportArtifact::default())?,
    ))
    .context("capacity_report.json has an incompatible capacity schema")?;
    validate_registry(&config.project_root)?;
    let resources = inputs.read("resource_profiles.json", json!({"scenarios": []}));
    let flamegraphs = inputs.read(
        "flamegraphs.json",
        fallback_flamegraphs(&config.output_dir.join("flamegraphs")),
    );
    let speed_report = inputs.read("speed_efficiency_report.json", json!({}));
    let frame_metrics = inputs.read("frame_metrics.json", json!({"scenarios": []}));
    let workflows = inputs.read("search_workflow_metrics.json", json!({"scenarios": []}));
    let latency_rows = unique_rows(
        values_array(&slowspots)
            .iter()
            .chain(values_array(&search))
            .chain(scenarios_array(&workflows))
            .cloned()
            .chain(frame_rows(&frame_metrics))
            .collect(),
    );
    let capacity_rows = capacity.scenarios;
    let resource_rows = scenarios_array(&resources);
    let capacity_synthetic = capacity.meta.synthetic;
    let resources_synthetic = payload_synthetic(&resources);
    let scenarios = review_scenarios();
    let mut scenario_payload: Vec<_> = scenarios
        .iter()
        .map(|scenario| {
            build_review_scenario(
                scenario,
                &latency_rows,
                &capacity_rows,
                resource_rows,
                values_array(&flamegraphs),
                capacity_synthetic,
                resources_synthetic,
            )
        })
        .collect();
    crate::baselines::annotate(config, &inputs.context, &mut scenario_payload)?;
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
            source_artifacts: source_status(config, &inputs.context),
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
            flamegraph_rows: values_array(&flamegraphs).len(),
            promise_health: PromiseHealthSummary {
                passing_but_regressed: scenario_payload.iter().filter(|row|row.promise_health == PromiseHealth::PassingButRegressed).count(),
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
// Only this report needs presentation aliases for raw frame evidence.
fn frame_rows(payload: &Value) -> Vec<Value> {
    scenarios_array(payload)
        .iter()
        .cloned()
        .map(|mut row| {
            let id = row["scenario_id"].as_str().unwrap_or("").to_owned();
            let key = shared::benchmark_key(&id);
            row["name"] = json!(id);
            row["benchmark_key"] = json!(key);
            row["evidence_kind"] = json!("frame");
            row["threshold_ms"] = row["budget_ms"].clone();
            row["threshold_authoritative"] = json!(true);
            row["measurement_status"] = json!(if valid_frame(&row) {
                "valid"
            } else {
                "invalid"
            });
            // Wheel-scroll is not the same family as component scroll benchmarks.
            if matches!(
                key,
                "ui_render_frame_120hz"
                    | "editor_scroll_frame_120hz"
                    | "editor_scroll_event_to_tessellation"
            ) {
                row["promise_scenarios"] = json!(["large_files"]);
            }
            row
        })
        .collect()
}

fn build_review_scenario(
    scenario: &ReviewScenario,
    latency: &[Value],
    capacity: &[CapacityScenario],
    resources: &[Value],
    flamegraphs: &[Value],
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
        .iter()
        .filter(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| scenario.profile_ids.contains(&id))
        })
        .cloned()
        .collect();
    let axes = coverage::axes(
        scenario,
        &latency_rows,
        &capacity_rows,
        &resource_rows,
        &profile_rows,
        capacity_synthetic,
        resources_synthetic,
    );
    let axis_values = [&axes.speed, &axes.capacity, &axes.resource, &axes.profiles];
    let required = axis_values.iter().filter(|axis| axis.required).count();
    let covered = axis_values
        .iter()
        .filter(|axis| axis.required && axis.covered)
        .count();
    let coverage_status = if required == covered {
        CoverageStatus::Covered
    } else if axis_values
        .iter()
        .any(|axis| axis.required && axis.available_count > 0)
    {
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
    let budget_misses = latency_rows
        .iter()
        .filter(|row| {
            scenario
                .benchmark_keys
                .contains(&row["benchmark_key"].as_str().unwrap_or(""))
                && crate::measurement_contract::eligible(
                    row["benchmark_key"].as_str().unwrap_or(""),
                    &row["measurement_contract"],
                )
                && over_budget_latency(row)
        })
        .count()
        + if resources_synthetic {
            0
        } else {
            resource_rows
                .iter()
                .filter(|row| resource_contract_failed(row))
                .count()
        };
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
        baseline_comparison: Default::default(),
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
                    .and_then(Value::as_i64)
                    .or_else(|| row.get("max_working_set_bytes").and_then(Value::as_i64))
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
    use super::{build_review_scenario, frame_rows, review_scenarios};
    use crate::artifacts::capacity_report::{CapacitySample, CapacityScenario};
    use crate::artifacts::performance_review::{
        CeilingStatus, CoverageStatus, PromiseHealth, ScaleCheckStatus,
    };
    use crate::shared;
    use serde_json::json;

    #[test]
    fn fresh_but_incompatible_capacity_is_not_silently_missing() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        let config = crate::config::LensConfig {
            project_name: "fixture".into(),
            project_root: root.path().into(),
            output_dir: root.path().into(),
        };
        let path = root.path().join("capacity_report.json");
        std::fs::write(&path, r#"{"scenarios":[{"scenario":"tab_count_ceiling"}]}"#)?;
        crate::provenance::record(
            &config,
            "capacity_report.json",
            0.0,
            &crate::provenance::context(root.path()),
            true,
        )?;
        let options = crate::cli::MeasureOptions {
            skip_bench: true,
            fail_on_slow: false,
            index_only: false,
        };
        let error = super::performance_review(&config, options).unwrap_err();
        assert!(error
            .to_string()
            .contains("capacity_report.json has an incompatible capacity schema"));
        assert!(!root.path().join("performance_review.json").exists());
        std::fs::remove_file(path)?;
        assert!(super::performance_review(&config, options)
            .unwrap_err()
            .to_string()
            .contains("missing project measurement manifest"));
        Ok(())
    }

    #[test]
    fn either_frame_tail_budget_changes_promise_health_and_keeps_scope() {
        let scenario = review_scenarios()
            .into_iter()
            .find(|s| s.id == "large_files")
            .unwrap();
        for (p95, p99, missed) in [(7.0, 10.0, false), (9.0, 10.0, true), (7.0, 20.0, true)] {
            let payload = json!({"scenarios":[{"scenario_id":"editor_scroll_event_to_tessellation/67108864",
                "measurement_contract":{"fixture_kind":"non_repeating","fixture_revision":2,"text_backing":"store","production_path":true,"code_path":"app","document_bytes":67108864},
                "measurement_scope":"end_to_end_event_to_tessellation", "present_included":false,
                "p95_ms":p95,"p99_ms":p99,"budget_ms":8.33,"p99_budget_ms":12,"frame_count":240}]});
            let row = build_review_scenario(
                &scenario,
                &frame_rows(&payload),
                &[],
                &[],
                &[],
                false,
                false,
            );
            assert_eq!(row.promise_health == PromiseHealth::Fail, missed);
            assert_eq!(row.evidence.latency[0]["present_included"], false);
        }
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
                "benchmark_key": "editor_scroll_frame_120hz",
                "measurement_contract":{"fixture_kind":"non_repeating","fixture_revision":2,"text_backing":"store","production_path":true,"code_path":"app","document_bytes":67108864},
                "workload_family": "file-load",
                "threshold_ms": 100.0,
                "budget_probe_ns": 125_000_000.0,
            })],
            &[CapacityScenario {
                scenario: "large_file_background_index_ceiling".into(),
                ceiling_reached: Some(false),
                samples: vec![CapacitySample {
                    workload_value: Some(shared::GB),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            &[json!({
                "scenario": "large_utf8_load_peak_memory",
                "samples": [{"workload_value": shared::GB}],
            })],
            &[json!({"id": "scroll_stress_profile", "available": true})],
            false,
            false,
        );
        assert_eq!(row.coverage_status, CoverageStatus::Thin);
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
            &[CapacityScenario {
                scenario: "tab_count_ceiling".into(),
                samples: vec![CapacitySample {
                    workload_value: Some(10_000),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            &[],
            &[],
            true,
            false,
        );
        assert_eq!(row.promise_health, PromiseHealth::Unmeasured);
        assert_eq!(row.scale_checks[0].status, ScaleCheckStatus::Unmeasured);
        assert!(!row.scale_checks[0].met);
    }

    #[test]
    fn partial_scale_evidence_and_other_operations_cannot_hide_early_failure() {
        let scenario = review_scenarios()
            .into_iter()
            .find(|s| s.id == "many_files")
            .unwrap();
        for early in [false, true] {
            let mut capacities = vec![CapacityScenario {
                scenario: "many_file_first_visible_ceiling".into(),
                ceiling_reached: Some(true),
                first_failure_workload: Some(if early { 5_000 } else { 50_000 }),
                last_successful_workload: Some(if early { 1_000 } else { 10_000 }),
                ..Default::default()
            }];
            if early {
                capacities.push(CapacityScenario {
                    scenario: "search_target_count_ceiling".into(),
                    ceiling_reached: Some(false),
                    last_successful_workload: Some(10_000),
                    samples: vec![CapacitySample {
                        workload_value: Some(10_000),
                        ..Default::default()
                    }],
                    ..Default::default()
                });
            }
            let row = build_review_scenario(
                &scenario,
                &[
                    json!({"benchmark_key":"search_current_completion_aggregate_size", "workload_family":"search", "threshold_ms":100.0, "budget_probe_ns":80_000_000.0}),
                ],
                &capacities,
                &[
                    json!({"scenario":"many_file_resource_tracking", "samples":[{"workload_value":10_000}]}),
                ],
                &[json!({"id":"search_all_tabs_profile", "available":true})],
                false,
                false,
            );
            assert_eq!(row.coverage_status, CoverageStatus::Thin);
            assert_eq!(
                row.promise_health,
                if early {
                    PromiseHealth::Fail
                } else {
                    PromiseHealth::AtRisk
                }
            );
            assert_eq!(row.ceilings_reached, 1);
            assert_eq!(row.scale_failures, usize::from(early));
            let check = &row.scale_checks[0];
            assert!(!check.met);
            assert_eq!(check.headroom_workload, None);
            assert_eq!(
                check.ceiling_status,
                if early {
                    CeilingStatus::FailedBeforePromise
                } else {
                    CeilingStatus::FailedAfterPromise
                }
            );
            if early {
                assert_eq!(
                    check.failed_capacity_scenarios,
                    ["many_file_first_visible_ceiling"]
                );
            }
        }
    }
}
