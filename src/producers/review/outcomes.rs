use super::registry::{review_scenarios, ReviewScenario};
use crate::{
    artifacts::{capacity_report::CapacityScenario, performance_review::GateOutcome},
    outcomes::{Outcome, SECTIONS},
};
use anyhow::Result;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn membership(
    id: &str,
    section: &str,
    definitions: &[ReviewScenario],
) -> (Vec<String>, Vec<Value>) {
    let mut shared = Vec::new();
    let mut scales = Vec::new();
    for p in definitions {
        let required = match section {
            "resource_budgets" => p.resource_scenarios,
            "capacity_budgets" => p.capacity_scenarios,
            _ => p.benchmark_keys,
        };
        let targets: Vec<_> = p
            .scale_targets
            .iter()
            .filter(|t| t.resource_scenarios.contains(&id) || t.capacity_scenarios.contains(&id))
            .collect();
        if required.contains(&id) || !targets.is_empty() {
            shared.push(p.id.to_owned());
        }
        scales.extend(
            targets
                .into_iter()
                .map(|t| json!({"promise":p.id,"id":t.id,"minimum":t.minimum,"unit":t.unit})),
        );
    }
    (shared, scales)
}

struct Evidence<'a> {
    observations: Vec<&'a Value>,
    memory: Vec<Value>,
    failed: bool,
    passed: bool,
}

fn observations(row: &Value) -> Vec<&Value> {
    let Some(samples) = row["samples"].as_array() else {
        return vec![&row["observed_outcome"]];
    };
    samples
        .iter()
        .flat_map(|s| {
            s["raw_repetitions"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(std::slice::from_ref(s))
        })
        .map(|s| &s["observed_outcome"])
        .collect()
}

impl<'a> Evidence<'a> {
    fn rows(rows: Vec<&'a Value>) -> Self {
        Self {
            observations: rows.iter().flat_map(|r| observations(r)).collect(),
            memory: rows
                .iter()
                .flat_map(|r| r["samples"].as_array().into_iter().flatten())
                .map(|s| s["memory_assessment"].clone())
                .collect(),
            failed: rows.iter().any(|r| {
                r["budget_status"] == "fail"
                    || (r["evidence_kind"] == "frame"
                        && crate::producers::common::frame_over_budget(r))
            }),
            passed: rows.iter().all(|r| {
                r["budget_status"] == "pass"
                    || (r["evidence_kind"] == "frame"
                        && crate::producers::common::valid_frame(r)
                        && !crate::producers::common::frame_over_budget(r))
            }),
        }
    }

    fn capacity(rows: &'a [CapacityScenario], id: &str, limit: Option<f64>) -> Self {
        let rows: Vec<_> = rows.iter().filter(|r| r.scenario == id).collect();
        let samples: Vec<_> = rows.iter().flat_map(|r| &r.samples).collect();
        Self {
            observations: samples
                .iter()
                .flat_map(|s| &s.raw_repetitions)
                .map(|r| &r["observed_outcome"])
                .collect(),
            memory: vec![Value::Null; samples.len()],
            failed: samples
                .iter()
                .any(|s| s.elapsed_ms.zip(limit).is_some_and(|(v, b)| v > b)),
            passed: rows.iter().all(|r| {
                !r.samples.is_empty()
                    && r.samples.iter().all(|s| {
                        s.complete
                            && s.failure_count == 0
                            && s.elapsed_ms.zip(limit).is_some_and(|(v, b)| v <= b)
                    })
            }),
        }
    }

    fn finish(
        self,
        id: &str,
        section: &str,
        budget: &Value,
        definitions: &[ReviewScenario],
    ) -> Result<GateOutcome> {
        let declaration = Outcome::deserialize(&budget["outcome"])?;
        let errors: BTreeSet<_> = self
            .observations
            .iter()
            .filter_map(|o| declaration.verify(o).err().map(|e| e.to_string()))
            .collect();
        let status = match (self.observations.is_empty(), errors.is_empty()) {
            (true, _) => "missing",
            (false, true) => "verified",
            _ => "invalid",
        };
        let verdict = match (self.failed, status, self.passed) {
            (true, _, _) => "fail",
            (_, "missing" | "invalid", _) => "unmeasured",
            (_, _, true) => "pass",
            _ => "uncertain",
        };
        let (shared_with, promised_scale) = membership(id, section, definitions);
        Ok(GateOutcome {
            id: id.into(),
            kind: section.into(),
            shared_with,
            declaration,
            budget: budget.clone(),
            promised_scale,
            verdict: verdict.into(),
            observation_status: status.into(),
            observation_errors: errors.into_iter().collect(),
            memory_assessments: self.memory,
        })
    }
}

pub(super) fn matrix(
    manifest: &Value,
    latency: &[Value],
    capacity: &[CapacityScenario],
    resources: &[Value],
) -> Result<Vec<GateOutcome>> {
    let definitions = review_scenarios();
    let mut result = Vec::new();
    for section in SECTIONS {
        for (id, budget) in manifest[section]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, b)| b.get("outcome").is_some())
        {
            let evidence = match section {
                "capacity_budgets" => {
                    Evidence::capacity(capacity, id, budget["threshold_ms"].as_f64())
                }
                _ => {
                    let rows = if section == "resource_budgets" {
                        resources
                    } else {
                        latency
                    };
                    Evidence::rows(
                        rows.iter()
                            .filter(|r| r["scenario"] == *id || r["benchmark_key"] == *id)
                            .filter(|r| {
                                section != "probe_measurements"
                                    || crate::measurement_contract::eligible(
                                        id,
                                        &r["measurement_contract"],
                                    )
                            })
                            .collect(),
                    )
                }
            };
            result.push(evidence.finish(id, section, budget, &definitions)?);
        }
    }
    Ok(result)
}

pub(super) fn markdown(rows: &[GateOutcome]) -> String {
    let mut text = String::from("# Promise outcomes\n\nGenerated from the project manifest. Review runs attach provenance-validated evidence; the standalone description has no measurements. Unmeasured is not a pass. CPU render endpoints exclude display presentation. These are measured-sweep budget verdicts; promise health also requires scale, coverage and noise checks. The JSON matrix contains one row per gate; the tables repeat shared memberships for readability.\n\n");
    for p in review_scenarios() {
        text.push_str(&format!("## {}\n\n| Gate | User outcome | Endpoint | Blocking | Residency | Durability | Budget | Verdict |\n|---|---|---|---|---|---|---|---|\n",p.title));
        for r in rows
            .iter()
            .filter(|r| r.shared_with.iter().any(|id| id == p.id))
        {
            let d = &r.declaration;
            let clean = |s: &str| s.replace('|', "\\|").replace('\n', " ");
            let budget = if let Some(ms) = r.budget["threshold_ms"].as_f64() {
                format!("{ms} ms")
            } else {
                format!(
                    "{} {}",
                    r.budget["max"],
                    r.budget["unit"].as_str().unwrap_or("")
                )
            };
            text.push_str(&format!(
                "| {} | {} | {} | {:?} | {:?} | {:?} | {} | {} ({}) |\n",
                r.id,
                clean(&d.user_outcome),
                clean(&d.endpoint),
                d.blocking,
                d.residency,
                d.durability_at_endpoint,
                budget,
                r.verdict,
                r.observation_status
            ));
        }
        text.push('\n');
    }
    text.push_str("## Supporting diagnostics without required promise membership\n\n");
    for r in rows.iter().filter(|r| r.shared_with.is_empty()) {
        text.push_str(&format!(
            "- {}: {} ({}; {})\n",
            r.id, r.declaration.user_outcome, r.declaration.endpoint, r.verdict
        ));
    }
    text.push_str("## Shared gate membership\n\n");
    for r in rows.iter().filter(|r| r.shared_with.len() > 1) {
        text.push_str(&format!(
            "- {} / {}: {}\n",
            r.kind,
            r.id,
            r.shared_with.join(", ")
        ));
    }
    text.push_str("\n## Memory and durability observations\n\nPending queue size is not total accepted uncommitted text. A NORMAL body flush does not establish FULL durability. Missing exposure or residency observations remain unknown.\n\n| Gate | Observed samples | Pending policy changed | FULL exposure observed | Missing memory metrics |\n|---|---|---|---|---|\n");
    for r in rows
        .iter()
        .filter(|r| r.memory_assessments.iter().any(Value::is_object))
    {
        let values: Vec<_> = r
            .memory_assessments
            .iter()
            .filter(|v| v.is_object())
            .collect();
        let changed = values.iter().filter(|v| v["policy_change"] == true).count();
        let known_policy = values
            .iter()
            .filter(|v| v["policy_change"].is_boolean())
            .count();
        let exposed = values
            .iter()
            .filter(|v| v["exposure_window_ms"].is_number())
            .count();
        let missing: std::collections::BTreeSet<_> = values
            .iter()
            .flat_map(|v| v["missing_metrics"].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect();
        text.push_str(&format!(
            "| {} | {} | {} changed / {} known | {} / {} | {} |\n",
            r.id,
            values.len(),
            changed,
            known_policy,
            exposed,
            values.len(),
            missing.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{markdown, matrix};
    use serde_json::{json, Value};
    #[test]
    fn resource_verdicts_keep_shared_membership_and_observation_validity() {
        let declaration = json!({"promise":"session_restore","user_outcome":"Persisted",
            "endpoint":"persist_complete","blocking":"background_completion","residency":"store_uncontrolled",
            "durability_at_endpoint":"committed_full","workload":"10k tabs"});
        let manifest = json!({"resource_budgets":{"session_persist_cost":{"outcome":declaration,"max":180000000,"unit":"ns"}}});
        let observed = json!({"completed":true,"endpoint":"persist_complete","residency":"store_uncontrolled",
            "durability_at_endpoint":"committed_full","manifest_commit_full_observed":true});
        for (status, observation, verdict) in [
            ("uncertain", &observed, "uncertain"),
            ("fail", &observed, "fail"),
            ("pass", &observed, "pass"),
            ("pass", &Value::Null, "unmeasured"),
        ] {
            let row = json!({"scenario":"session_persist_cost","budget_status":status,
                "samples":[{"budget_status":"fail","observed_outcome":observation,
                "memory_assessment":{"policy_change":false,"exposure_window_ms":null,"missing_metrics":["scratch_peak_bytes"]}}]});
            let rows = matrix(&manifest, &[], &[], &[row]).unwrap();
            assert_eq!(rows.len(), 1);
            assert!(rows[0].shared_with.len() > 1);
            assert_eq!(rows[0].verdict, verdict);
            let document = markdown(&rows);
            assert!(document.contains("scratch_peak_bytes"));
            assert!(document.contains("0 changed / 1 known"));
        }
        let capacity_manifest = json!({"capacity_budgets":{"session_persist_cost":{
            "outcome":manifest["resource_budgets"]["session_persist_cost"]["outcome"],"threshold_ms":180}}});
        for (elapsed, complete, failures, verdict) in [
            (Some(100.0), true, 0, "pass"),
            (Some(200.0), true, 0, "fail"),
            (Some(100.0), false, 0, "uncertain"),
            (Some(100.0), true, 1, "uncertain"),
            (None, true, 0, "uncertain"),
        ] {
            let rows = [crate::artifacts::capacity_report::CapacityScenario {
                scenario: "session_persist_cost".into(),
                samples: vec![crate::artifacts::capacity_report::CapacitySample {
                    elapsed_ms: elapsed,
                    complete,
                    failure_count: failures,
                    raw_repetitions: vec![json!({"observed_outcome":observed})],
                    ..Default::default()
                }],
                ..Default::default()
            }];
            assert_eq!(
                matrix(&capacity_manifest, &[], &rows, &[]).unwrap()[0].verdict,
                verdict
            );
        }
        let mut declaration_only = manifest;
        declaration_only["resource_budgets"]["undeclared"] = json!({});
        assert_eq!(
            matrix(&declaration_only, &[], &[], &[]).unwrap()[0].observation_status,
            "missing"
        );
    }

    #[test]
    fn frame_outcomes_preserve_tail_failure_and_exclude_owned_references() {
        let declaration = json!({"promise":"large_files","user_outcome":"Frame measured",
            "endpoint":"measured_frame_path_complete","blocking":"foreground_interaction",
            "residency":"store_uncontrolled","durability_at_endpoint":"not_required","workload":"1 GiB"});
        let manifest = json!({"probe_measurements":{"editor_scroll_frame_120hz":{
            "threshold_ms":8.33,"outcome":declaration}}});
        let store = json!({"benchmark_key":"editor_scroll_frame_120hz","evidence_kind":"frame",
            "p95_ms":1,"p99_ms":2,"budget_ms":8.33,"p99_budget_ms":8.33,"frame_count":128,
            "measurement_scope":"frame","measurement_contract":{"fixture_kind":"non_repeating",
            "fixture_revision":1,"production_path":true,"text_backing":"store","code_path":"editor",
            "document_bytes":1073741824},"observed_outcome":{"completed":true,
            "endpoint":"measured_frame_path_complete","residency":"store_uncontrolled","durability_at_endpoint":"not_required"}});
        let mut owned = store.clone();
        owned["measurement_contract"]["text_backing"] = json!("owned");
        owned["observed_outcome"]["residency"] = json!("owned");
        assert_eq!(
            matrix(&manifest, std::slice::from_ref(&owned), &[], &[]).unwrap()[0].verdict,
            "unmeasured"
        );
        assert_eq!(
            matrix(&manifest, &[store.clone(), owned], &[], &[]).unwrap()[0].verdict,
            "pass"
        );
        let mut slow = store;
        slow["p99_ms"] = json!(10);
        assert_eq!(
            matrix(&manifest, &[slow], &[], &[]).unwrap()[0].verdict,
            "fail"
        );
    }
}
