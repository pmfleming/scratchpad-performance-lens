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

/// Complete local controls supersede a producer-wide interval only for their rows.
/// Legacy evidence keeps the producer-wide fallback; partial controls stay uncertain.
fn locally_unstable(rows: &[&Value], producer_noisy: bool) -> bool {
    if rows.is_empty()
        || rows
            .iter()
            .all(|row| row.get("launch_measurement_quality").is_none())
    {
        return producer_noisy;
    }
    let mut workloads: std::collections::BTreeMap<String, Vec<&Value>> = Default::default();
    for row in rows {
        workloads
            .entry(row["workload_value"].to_string())
            .or_default()
            .push(row);
    }
    workloads.values().any(|group| {
        let Some(expected) = group[0]["independent_launch_count"]
            .as_u64()
            .filter(|n| (1..=16).contains(n))
        else {
            return true;
        };
        !super::super::common::complete_launches(group, expected)
            || group.iter().any(|row| {
                let control = &row["launch_measurement_quality"];
                control["scope"] != "independent_probe_launch_including_setup"
                    || crate::environment::quality(&control["before"], &control["after"])["status"]
                        != "stable"
            })
    })
}

fn uses_resource(scenario: &ReviewScenario, id: &str) -> bool {
    scenario.resource_scenarios.contains(&id)
        || scenario
            .scale_targets
            .iter()
            .any(|target| target.resource_scenarios.contains(&id))
}
fn uses_capacity(scenario: &ReviewScenario, id: &str) -> bool {
    scenario.capacity_scenarios.contains(&id)
        || scenario
            .scale_targets
            .iter()
            .any(|target| target.capacity_scenarios.contains(&id))
}

pub(super) fn relevant_sources<'a>(
    scenario: &ReviewScenario,
    noisy: &[String],
    latency: &[(&'a str, &[Value])],
    capacity: &[CapacityScenario],
    resources: &[Value],
) -> Vec<&'a str> {
    let producer_noisy = |id: &str| noisy.iter().any(|source| source == id);
    let mut result: Vec<_> = latency
        .iter()
        .filter(|(id, rows)| {
            is_measurement_source(id)
                && producer_noisy(id)
                && rows.iter().any(|row| review_row_matches(row, scenario))
        })
        .map(|(id, _)| *id)
        .collect();
    if resources
        .iter()
        .filter(|row| {
            row["scenario"]
                .as_str()
                .is_some_and(|id| uses_resource(scenario, id))
        })
        .any(|row| {
            let samples: Vec<_> = row["samples"].as_array().into_iter().flatten().collect();
            locally_unstable(&samples, producer_noisy("resources"))
        })
    {
        result.push("resources");
    }
    if capacity
        .iter()
        .filter(|row| uses_capacity(scenario, &row.scenario))
        .any(|row| {
            if row.samples.is_empty() {
                return producer_noisy("capacity");
            }
            row.samples.iter().any(|sample| {
                let repetitions: Vec<_> = sample.raw_repetitions.iter().collect();
                locally_unstable(&repetitions, producer_noisy("capacity"))
            })
        })
    {
        result.push("capacity");
    }
    result
}

pub(super) fn flag(scenario: &mut PerformanceReviewScenario, sources: &[&str]) {
    if matches!(
        scenario.promise_health,
        PromiseHealth::Pass | PromiseHealth::PassingButRegressed
    ) {
        scenario.promise_health = PromiseHealth::AtRisk;
    }
    scenario.opportunities = vec![
        "Repeat measurements with complete stable controls before claiming this promise passes."
            .into(),
    ];
    scenario.baseline_comparison.status = RegressionStatus::Noisy;
    scenario.gaps.push(format!(
        "Measurement controls are noisy or incomplete in: {}",
        sources.join(", ")
    ));
}

#[cfg(test)]
mod tests {
    use super::{flag, is_measurement_source, locally_unstable, relevant_sources};
    use crate::artifacts::performance_review::PromiseHealth;
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
        assert!(!row
            .opportunities
            .iter()
            .any(|s| s.contains("currently passing")));
        row.promise_health = PromiseHealth::Fail;
        flag(&mut row, &["search_workflows"]);
        assert_eq!(row.promise_health, PromiseHealth::Fail);
    }
    #[test]
    fn local_controls_require_every_launch_and_do_not_hide_failures() {
        let control = |n| json!({"median_ns":n,"min_ns":n,"max_ns":n});
        let mut quality = crate::environment::quality(&control(100), &control(100));
        quality["scope"] = json!("independent_probe_launch_including_setup");
        let mut rows: Vec<_> = (0..3)
            .map(|index| {
                json!({"workload_value":100,"launch_index":index,
            "independent_launch_count":3,"launch_measurement_quality":quality})
            })
            .collect();
        assert!(!locally_unstable(&rows.iter().collect::<Vec<_>>(), true));
        assert!(locally_unstable(
            &rows[..2].iter().collect::<Vec<_>>(),
            false
        ));
        rows[2]["launch_measurement_quality"]["after"] = control(130);
        assert!(locally_unstable(&rows.iter().collect::<Vec<_>>(), false));
        rows[2]
            .as_object_mut()
            .unwrap()
            .remove("launch_measurement_quality");
        assert!(locally_unstable(&rows.iter().collect::<Vec<_>>(), false));
        let legacy = json!({"workload_value":100});
        assert!(locally_unstable(&[&legacy], true));
        assert!(!locally_unstable(&[&legacy], false));
    }

    #[test]
    fn noisy_resource_launch_only_flags_its_dependent_promises() {
        let definitions = review_scenarios();
        let views = definitions
            .iter()
            .find(|row| row.id == "many_views")
            .unwrap();
        let control = |n| json!({"median_ns":n,"min_ns":n,"max_ns":n});
        let sample = |n| {
            let mut quality = crate::environment::quality(&control(100), &control(n));
            quality["scope"] = json!("independent_probe_launch_including_setup");
            json!({"workload_value":100,"launch_index":0,"independent_launch_count":1,"launch_measurement_quality":quality})
        };
        let mut resources = vec![
            json!({"scenario":"anchor_heavy_view_editing","samples":[sample(100)]}),
            json!({"scenario":"session_persist_cost","samples":[sample(130)]}),
        ];
        assert!(relevant_sources(views, &["resources".into()], &[], &[], &resources).is_empty());
        resources[0]["samples"][0] = sample(130);
        assert_eq!(
            relevant_sources(views, &[], &[], &[], &resources),
            ["resources"]
        );
        resources[0]["samples"][0]["launch_measurement_quality"] = json!({});
        assert_eq!(
            relevant_sources(views, &[], &[], &[], &resources),
            ["resources"]
        );
    }
}
