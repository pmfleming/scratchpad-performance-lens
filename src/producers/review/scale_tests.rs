use super::health::scale_checks;
use super::registry::review_scenarios;
use crate::artifacts::capacity_report::{CapacitySample, CapacityScenario};

#[test]
fn failed_attempt_above_target_is_not_success_or_headroom() {
    let scenario = review_scenarios()
        .into_iter()
        .find(|s| s.id == "many_tabs")
        .unwrap();
    let mut capacity = CapacityScenario {
        scenario: "tab_count_ceiling".into(),
        threshold_ms: 140.0,
        first_failure_workload: Some(20_000),
        last_successful_workload: Some(4096),
        samples: vec![
            CapacitySample {
                workload_value: Some(4096),
                elapsed_ms: Some(5.0),
                complete: true,
                repetition_count: 3,
                status: "ok".into(),
                ..Default::default()
            },
            CapacitySample {
                workload_value: Some(20_000),
                elapsed_ms: Some(200.0),
                complete: true,
                repetition_count: 3,
                status: "ok".into(),
                ..Default::default()
            },
        ],
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
fn session_promise_uses_real_resource_output_and_separate_latency_budgets() {
    use crate::artifacts::performance_review::PromiseHealth;
    use serde_json::json;
    let scenario = review_scenarios()
        .into_iter()
        .find(|s| s.id == "session_restore")
        .unwrap();
    for (case, expected) in [
        ("valid", PromiseHealth::Pass),
        ("panic", PromiseHealth::Fail),
        ("missing_metric", PromiseHealth::AtRisk),
        ("undersized", PromiseHealth::AtRisk),
        ("incomplete_history", PromiseHealth::AtRisk),
        ("slow", PromiseHealth::Fail),
        ("slow_initial_save", PromiseHealth::Fail),
    ] {
        let mut events: Vec<_> = scenario.resource_scenarios.iter().map(|id| json!({
            "scenario":id,"status":"ok","resource_budget":{"metric":"/elapsed_ns","max":180000000,"authoritative":true},"elapsed_ns":1_000_000,"measurement_scope":"prepared_operation",
            "workload_value":if *id == "large_dirty_buffer_save" {50 * crate::shared::MB} else {10_000},
            "allocated_bytes":1,"manifest_size_bytes":1,
            "phase_elapsed_ns":{"snapshot_capture_ns":1,"file_write_ns":1,"manifest_serialize_ns":1,
                "manifest_read_parse_ns":1,"restore_reconstruction_ns":1,"startup_load_ns":1,"first_paint_ns":1,"save_ns":1},
            "process_resources":{"write_bytes":1},
            "workload_details":{"history_payload_bytes":16 * crate::shared::MB,"history_entries":1024,"inserted_bytes":1,"synchronous":"FULL"}
        })).collect();
        match case {
            "slow_initial_save" => events[0]["elapsed_ns"] = json!(5_200_000_000u64),
            "panic" => events[2]["status"] = json!("panic"),
            "missing_metric" => {
                events
                    .iter_mut()
                    .find(|e| e["scenario"] == "session_restore_cost")
                    .unwrap()["phase_elapsed_ns"]["manifest_read_parse_ns"] = json!(null)
            }
            "undersized" => events[2]["workload_value"] = json!(9999),
            "incomplete_history" => {
                events
                    .iter_mut()
                    .find(|e| e["scenario"] == "large_dirty_buffer_save")
                    .unwrap()["workload_details"]["history_payload_bytes"] = json!(0)
            }
            _ => {}
        }
        let resources = super::super::resources::summarize_resources(events, "completed");
        let latency: Vec<_> = scenario
            .benchmark_keys
            .iter()
            .map(|key| {
                json!({
                    "benchmark_key":key,"threshold_ms":1,"threshold_authoritative":true,
                    "measurement_scope":"prepared_operation","measurement_status":"valid",
                    "budget_probe_ns":if case == "slow" {2_000_000} else {1000},
                    "budget_status":if case == "slow" {"fail"} else {"pass"},
                })
            })
            .collect();
        let row = super::build_review_scenario(
            &scenario,
            &latency,
            &[],
            resources["scenarios"].as_array().unwrap(),
            &[],
            false,
            false,
        );
        assert_eq!(row.promise_health, expected, "{case}");
        if case == "valid" {
            assert!(row.scale_checks.iter().all(|check| check.met));
            assert_eq!(row.scale_checks[1].last_successful_workload, Some(10_000));
        }
    }
}
