use super::health::scale_checks;
use super::registry::review_scenarios;
use crate::artifacts::capacity_report::{CapacitySample, CapacityScenario};

#[test]
fn failed_attempt_above_target_is_not_success_or_headroom() {
    let scenario = review_scenarios().into_iter().find(|s| s.id == "many_tabs").unwrap();
    let mut capacity = CapacityScenario {
        scenario: "tab_count_ceiling".into(), threshold_ms: 140.0,
        first_failure_workload: Some(20_000), last_successful_workload: Some(4096),
        samples: vec![CapacitySample { workload_value: Some(4096), elapsed_ms: Some(5.0), complete: true,
            repetition_count: 3, status: "ok".into(), ..Default::default() },
            CapacitySample { workload_value: Some(20_000), elapsed_ms: Some(200.0), complete: true,
                repetition_count: 3, status: "ok".into(), ..Default::default() }],
        ..Default::default()
    };
    let check = &scale_checks(&scenario, &[capacity.clone()], &[], false, false)[0];
    assert!(!check.met);
    assert_eq!(check.last_successful_workload, Some(4096));
    assert_eq!(check.headroom_workload, None);
    capacity.samples[0].workload_value = Some(12_000);
    let check = &scale_checks(&scenario, &[capacity], &[], false, false)[0];
    assert!(check.met);
    assert_eq!(check.headroom_workload, Some(2000));
}

#[test]
fn failed_resource_sample_does_not_demonstrate_restore_scale() {
    let scenario = review_scenarios().into_iter().find(|s| s.id == "session_restore").unwrap();
    let resources = serde_json::json!({"scenario": "session_restore_cost", "threshold_ms": 100,
        "samples": [{"workload_value": 10000, "elapsed_ms": 1, "status": "panic"}]});
    assert!(!scale_checks(&scenario, &[], &[resources], false, false)[0].met);
}
