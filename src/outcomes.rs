//! Declared outcomes and independently observed completion evidence.
use anyhow::{ensure, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

pub const SECTIONS: [&str; 3] = ["resource_budgets", "capacity_budgets", "probe_measurements"];

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Blocking {
    ForegroundInteraction,
    BackgroundCompletion,
    InteractionDuringBackground,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Residency {
    StoreCold,
    StoreWarm,
    StoreResident,
    StoreUncontrolled,
    Owned,
    NotApplicable,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Durability {
    NotRequired,
    PublishedRam,
    CommittedNormal,
    CommittedFull,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub promise: String,
    pub user_outcome: String,
    pub endpoint: String,
    pub blocking: Blocking,
    pub residency: Residency,
    pub durability_at_endpoint: Durability,
    pub workload: String,
}

pub fn load(root: &Path) -> Result<Value> {
    let manifest = serde_json::from_slice(&std::fs::read(
        root.join("benches/measurement_manifest.json"),
    )?)?;
    validate(&manifest)?;
    Ok(manifest)
}

pub fn validate(manifest: &Value) -> Result<()> {
    ensure!(
        manifest["outcome_contract_version"] == 1,
        "missing or unsupported outcome_contract_version"
    );
    for section in SECTIONS {
        let entries = manifest[section]
            .as_object()
            .with_context(|| format!("missing {section}"))?;
        for (id, entry) in entries {
            let outcome: Outcome = Outcome::deserialize(&entry["outcome"])
                .with_context(|| format!("{section}/{id}: missing or invalid outcome"))?;
            ensure!(
                [
                    "large_files",
                    "many_files",
                    "search",
                    "many_tabs",
                    "many_views",
                    "text_mutation",
                    "session_restore"
                ]
                .contains(&outcome.promise.as_str()),
                "{id}: unknown promise"
            );
            ensure!(
                [
                    &outcome.promise,
                    &outcome.user_outcome,
                    &outcome.endpoint,
                    &outcome.workload
                ]
                .iter()
                .all(|s| !s.trim().is_empty()),
                "{id}: empty outcome field"
            );
        }
    }
    Ok(())
}

/// A label never proves residency, publication, or power-loss durability.
pub fn verify(declaration: &Value, observed: &Value) -> Result<()> {
    Outcome::deserialize(declaration)?.verify(observed)
}

impl Outcome {
    pub fn verify(&self, observed: &Value) -> Result<()> {
        ensure!(observed["completed"] == true, "completion was not observed");
        ensure!(
            observed["endpoint"].as_str() == Some(self.endpoint.as_str()),
            "observed endpoint differs from declaration"
        );
        ensure!(
            Residency::deserialize(&observed["residency"])? == self.residency,
            "observed residency differs from declaration"
        );
        ensure!(
            Durability::deserialize(&observed["durability_at_endpoint"])?
                == self.durability_at_endpoint,
            "observed durability_at_endpoint differs from declaration"
        );
        match self.residency {
            Residency::StoreResident => ensure!(
                observed["store_read_count"] == 0
                    && observed["retained_read_count"]
                        .as_u64()
                        .is_some_and(|n| n > 0)
                    && observed["app_retention_policy"] == true,
                "resident claim lacks cache/policy proof"
            ),
            Residency::StoreWarm => ensure!(
                observed["app_cache_empty_at_start"] == true && observed["os_cache_primed"] == true,
                "warm claim lacks setup proof"
            ),
            Residency::StoreCold => ensure!(
                observed["eviction_succeeded"] == true
                    && observed["physical_read_bytes"]
                        .as_u64()
                        .is_some_and(|n| n > 0),
                "cold claim lacks physical read proof"
            ),
            _ => {}
        }
        match self.durability_at_endpoint {
            Durability::PublishedRam => ensure!(
                observed["publication_observed"] == true,
                "publication not observed"
            ),
            Durability::CommittedNormal => ensure!(
                observed["body_commit_normal_observed"] == true,
                "NORMAL body commit not observed"
            ),
            Durability::CommittedFull => ensure!(
                observed["manifest_commit_full_observed"] == true
                    || observed["previous_full_snapshot_reused"] == true,
                "FULL manifest durability not observed"
            ),
            Durability::NotRequired => {}
        }
        Ok(())
    }
}

/// Historical unversioned fixtures retain their old contract. Versioned producers
/// cannot fall back to it when observations are absent.
pub fn annotate(manifest: &Value, section: &str, id: &str, event: &mut Value) {
    if manifest.get("outcome_contract_version").is_none() {
        return;
    }
    let declaration = &manifest[section][id]["outcome"];
    let result = verify(declaration, &event["observed_outcome"]);
    event["declared_outcome"] = declaration.clone();
    event["outcome_valid"] = json!(result.is_ok());
    event["outcome_error"] = json!(result.err().map(|e| e.to_string()));
}

#[cfg(test)]
mod tests {
    use super::{validate, verify};
    use serde_json::{json, Value};
    fn declaration() -> Value {
        json!({"promise":"search","user_outcome":"all matches returned",
        "endpoint":"all_ranges_returned","blocking":"background_completion","residency":"store_resident",
        "durability_at_endpoint":"not_required","workload":"non-repeating UTF-8"})
    }
    #[test]
    fn resident_labels_require_observations_and_production_policy() {
        let d = declaration();
        let mut o = json!({"completed":true,"endpoint":"all_ranges_returned","residency":"store_resident",
            "durability_at_endpoint":"not_required","store_read_count":0,"retained_read_count":10,"app_retention_policy":true});
        assert!(verify(&d, &o).is_ok());
        for field in [
            "completed",
            "endpoint",
            "store_read_count",
            "retained_read_count",
            "app_retention_policy",
        ] {
            let mut missing = o.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(verify(&d, &missing).is_err(), "{field}");
        }
        o["store_read_count"] = json!(1);
        assert!(verify(&d, &o).is_err());
    }
    #[test]
    fn normal_commit_cannot_prove_full_durability() {
        let mut d = declaration();
        d["residency"] = json!("owned");
        d["durability_at_endpoint"] = json!("committed_full");
        let mut o = json!({"completed":true,"endpoint":"all_ranges_returned","residency":"owned",
            "durability_at_endpoint":"committed_full","body_commit_normal_observed":true});
        assert!(verify(&d, &o).is_err());
        o["manifest_commit_full_observed"] = json!(true);
        assert!(verify(&d, &o).is_ok());
    }
    #[test]
    fn malformed_or_missing_outcomes_are_manifest_errors() {
        let mut m = json!({"outcome_contract_version":1,"resource_budgets":{"read":{"outcome":declaration()}},"capacity_budgets":{},"probe_measurements":{}});
        assert!(validate(&m).is_ok());
        m["resource_budgets"]["read"]["outcome"]["residency"] = json!("probably_warm");
        assert!(validate(&m).is_err());
        m["resource_budgets"]["read"]["outcome"] = Value::Null;
        assert!(validate(&m).is_err());
    }
}
