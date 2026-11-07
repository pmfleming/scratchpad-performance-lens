use super::registry::review_scenarios;
use crate::outcomes::{Outcome, SECTIONS};
use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GateOutcome {
    pub id: String,
    pub kind: String,
    pub shared_with: Vec<String>,
    pub declaration: Outcome,
    pub budget: Value,
    pub promised_scale: Vec<Value>,
    pub verdict: String,
    pub observation_status: String,
    pub observation_errors: Vec<String>,
    pub memory_assessments: Vec<Value>,
}

pub(super) fn matrix(
    manifest: &Value,
    latency: &[Value],
    capacity: &[Value],
    resources: &[Value],
) -> Result<Vec<GateOutcome>> {
    let definitions = review_scenarios();
    let mut result = Vec::new();
    for section in SECTIONS {
        for (id, budget) in manifest[section].as_object().into_iter().flatten() {
            let Some(declaration) = budget.get("outcome") else {
                continue;
            };
            let declaration: Outcome = serde_json::from_value(declaration.clone())?;
            let mut shared_with = Vec::new();
            let mut scale = Vec::new();
            for p in &definitions {
                let used = match section {
                    "resource_budgets" => p.resource_scenarios.contains(&id.as_str()),
                    "capacity_budgets" => p.capacity_scenarios.contains(&id.as_str()),
                    _ => p.benchmark_keys.contains(&id.as_str()),
                };
                if used {
                    shared_with.push(p.id.to_owned());
                }
                for t in p.scale_targets {
                    if t.resource_scenarios.contains(&id.as_str())
                        || t.capacity_scenarios.contains(&id.as_str())
                    {
                        if !shared_with.iter().any(|s| s == p.id) {
                            shared_with.push(p.id.to_owned());
                        }
                        scale.push(
                            json!({"promise":p.id,"id":t.id,"minimum":t.minimum,"unit":t.unit}),
                        );
                    }
                }
            }
            let rows = match section {
                "resource_budgets" => resources,
                "capacity_budgets" => capacity,
                _ => latency,
            };
            let matching: Vec<_> = rows
                .iter()
                .filter(|r| r["scenario"] == *id || r["benchmark_key"] == *id)
                .filter(|r| {
                    section != "probe_measurements"
                        || crate::measurement_contract::eligible(id, &r["measurement_contract"])
                })
                .collect();
            let mut observations = Vec::new();
            for row in &matching {
                if let Some(samples) = row["samples"].as_array() {
                    for sample in samples {
                        if let Some(raw) = sample["raw_repetitions"].as_array() {
                            observations.extend(raw.iter().map(|r| &r["observed_outcome"]));
                        } else {
                            observations.push(&sample["observed_outcome"]);
                        }
                    }
                } else {
                    observations.push(&row["observed_outcome"]);
                }
            }
            let errors: std::collections::BTreeSet<_> = observations
                .iter()
                .filter_map(|o| {
                    crate::outcomes::verify(&budget["outcome"], o)
                        .err()
                        .map(|e| e.to_string())
                })
                .collect();
            let observed = !observations.is_empty() && errors.is_empty();
            let failed = matching.iter().any(|r| {
                r["budget_status"] == "fail"
                    || (r["evidence_kind"] == "frame"
                        && crate::producers::common::frame_over_budget(r))
                    || r["samples"].as_array().is_some_and(|samples| {
                        samples.iter().any(|s| {
                            s["budget_status"] == "fail"
                                || s["elapsed_ms"]
                                    .as_f64()
                                    .zip(budget["threshold_ms"].as_f64())
                                    .is_some_and(|(v, b)| v > b)
                        })
                    })
            });
            let verdict = if failed {
                "fail"
            } else if !observed {
                "unmeasured"
            } else if matching.iter().all(|r| {
                r["budget_status"] == "pass"
                    || (r["evidence_kind"] == "frame"
                        && crate::producers::common::valid_frame(r)
                        && !crate::producers::common::frame_over_budget(r))
                    || (section == "capacity_budgets"
                        && r["samples"].as_array().is_some_and(|samples| {
                            !samples.is_empty()
                                && samples.iter().all(|s| {
                                    s["complete"] == true
                                        && s["failure_count"] == 0
                                        && s["elapsed_ms"]
                                            .as_f64()
                                            .zip(budget["threshold_ms"].as_f64())
                                            .is_some_and(|(v, b)| v <= b)
                                })
                        }))
            }) {
                "pass"
            } else {
                "uncertain"
            };
            result.push(GateOutcome {
                id: id.clone(),
                kind: section.into(),
                shared_with,
                declaration,
                budget: budget.clone(),
                promised_scale: scale,
                verdict: verdict.into(),
                observation_status: if observed {
                    "verified"
                } else if observations.is_empty() {
                    "missing"
                } else {
                    "invalid"
                }
                .into(),
                observation_errors: errors.into_iter().collect(),
                memory_assessments: matching
                    .iter()
                    .flat_map(|r| r["samples"].as_array().into_iter().flatten())
                    .map(|s| s["memory_assessment"].clone())
                    .collect(),
            });
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
            .filter(|r| r.shared_with.contains(&p.id.to_owned()))
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
    use super::*;
    #[test]
    fn shared_gate_is_one_row_and_missing_observations_cannot_pass() {
        let m = json!({"resource_budgets":{"session_persist_cost":{"max":180000000,"unit":"ns","outcome":{
            "promise":"session_restore","user_outcome":"persisted","endpoint":"persist_complete",
            "blocking":"background_completion","residency":"store_uncontrolled","durability_at_endpoint":"committed_full","workload":"10k tabs"}}}});
        let rows = matrix(
            &m,
            &[],
            &[],
            &[json!({"scenario":"session_persist_cost","budget_status":"pass"})],
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].shared_with.len() > 1);
        assert_eq!(rows[0].verdict, "unmeasured");
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
            matrix(&manifest, &[owned.clone()], &[], &[]).unwrap()[0].verdict,
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
