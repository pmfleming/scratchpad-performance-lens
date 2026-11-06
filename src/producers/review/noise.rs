//! Measurement controls apply to the promises that use those measured rows.
use super::{evidence::review_row_matches, registry::ReviewScenario};
use crate::artifacts::{
    capacity_report::CapacityScenario,
    performance_review::{PerformanceReviewScenario, PromiseHealth, RegressionStatus},
};
use serde_json::Value;

pub(super) fn is_measurement_source(id: &str) -> bool {
    matches!(
        id,
        "slowspots"
            | "search_speed"
            | "frame_metrics"
            | "search_workflows"
            | "capacity"
            | "resources"
    )
}

pub(super) fn relevant_sources<'a>(
    scenario: &ReviewScenario,
    noisy: &'a [String],
    latency: &[(&str, &[Value])],
    capacity: &[CapacityScenario],
    resources: &[Value],
) -> Vec<&'a str> {
    noisy
        .iter()
        .filter(|id| match id.as_str() {
            "capacity" => capacity.iter().any(|row| {
                scenario.capacity_scenarios.contains(&row.scenario.as_str())
                    || scenario
                        .scale_targets
                        .iter()
                        .any(|target| target.capacity_scenarios.contains(&row.scenario.as_str()))
            }),
            "resources" => resources.iter().any(|row| {
                row["scenario"].as_str().is_some_and(|id| {
                    scenario.resource_scenarios.contains(&id)
                        || scenario
                            .scale_targets
                            .iter()
                            .any(|target| target.resource_scenarios.contains(&id))
                })
            }),
            id if is_measurement_source(id) => latency.iter().any(|(source, rows)| {
                *source == id && rows.iter().any(|row| review_row_matches(row, scenario))
            }),
            _ => false,
        })
        .map(String::as_str)
        .collect()
}

pub(super) fn flag(scenario: &mut PerformanceReviewScenario, sources: &[&str]) {
    if matches!(
        scenario.promise_health,
        PromiseHealth::Pass | PromiseHealth::PassingButRegressed
    ) {
        scenario.promise_health = PromiseHealth::AtRisk;
    }
    scenario.baseline_comparison.status = RegressionStatus::Noisy;
    scenario.gaps.push(format!(
        "Measurement control detected noise in: {}",
        sources.join(", ")
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::producers::review::{build_review_scenario, registry::review_scenarios};
    use serde_json::json;

    #[test]
    fn noise_follows_measured_dependencies_and_never_a_derived_report() {
        let definitions = review_scenarios();
        let search = definitions.iter().find(|row| row.id == "search").unwrap();
        let views = definitions
            .iter()
            .find(|row| row.id == "many_views")
            .unwrap();
        let workflows =
            [json!({"benchmark_key":"search_workflow_all_completion","workload_family":"search"})];
        let noisy = vec!["search_workflows".to_owned(), "speed_report".to_owned()];
        let sources = [("search_workflows", workflows.as_slice())];
        assert_eq!(
            relevant_sources(search, &noisy, &sources, &[], &[]),
            ["search_workflows"]
        );
        assert!(relevant_sources(views, &noisy, &sources, &[], &[]).is_empty());
        assert!(!is_measurement_source("speed_report"));
        let resources = [json!({"scenario":"anchor_heavy_view_editing"})];
        assert_eq!(
            relevant_sources(views, &["resources".into()], &[], &[], &resources),
            ["resources"]
        );
        let mut row = build_review_scenario(search, &[], &[], &[], &[], false, false);
        row.promise_health = PromiseHealth::Pass;
        flag(&mut row, &["search_workflows"]);
        assert_eq!(row.promise_health, PromiseHealth::AtRisk);
        row.promise_health = PromiseHealth::Fail;
        flag(&mut row, &["search_workflows"]);
        assert_eq!(row.promise_health, PromiseHealth::Fail);
    }
}
