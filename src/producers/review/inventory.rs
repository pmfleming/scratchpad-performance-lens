//! Project-owned inventory validation is separate from the promise definitions.
use super::registry::review_scenarios;
use crate::shared;
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{collections::HashMap, path::Path};

struct Inventory<'a> {
    root: &'a Path,
    manifest: Value,
    metadata: HashMap<String, Value>,
}

impl Inventory<'_> {
    fn probe_exists(&self, name: &str) -> bool {
        self.root.join(format!("src/bin/{name}.rs")).is_file()
    }
    fn benchmark_budgeted(&self, key: &str) -> bool {
        let criterion = self.manifest["suites"]
            .as_object()
            .into_iter()
            .flat_map(|s| s.values())
            .any(|groups| groups[key].as_array().is_some_and(|p| !p.is_empty()));
        if criterion {
            return positive(
                self.metadata
                    .get(key)
                    .and_then(shared::authoritative_threshold_ms),
            );
        }
        let probe = &self.manifest["probe_measurements"][key];
        probe["probe"]
            .as_str()
            .is_some_and(|name| self.probe_exists(name))
            && positive(probe["threshold_ms"].as_f64())
    }
    fn probe_errors(&self, kind: &str, ids: &[&str]) -> Vec<String> {
        let probe = format!("{kind}_probe");
        let budgets = format!("{kind}_budgets");
        let limit = if kind == "capacity" {
            "threshold_ms"
        } else {
            "max"
        };
        let mut errors = Vec::new();
        if !ids.is_empty() && !self.probe_exists(&probe) {
            errors.push(format!("missing {probe} target"));
        }
        for id in ids {
            let declared = self.manifest["probes"][&probe]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row == *id));
            if !declared || !positive(self.manifest[&budgets][*id][limit].as_f64()) {
                errors.push(format!("{id} is absent or unbudgeted"));
            }
        }
        errors
    }
}

fn positive(value: Option<f64>) -> bool {
    value.is_some_and(|value| value.is_finite() && value > 0.0)
}

pub fn validate(root: &Path) -> Result<()> {
    let inventory = Inventory {
        root,
        metadata: shared::load_benchmark_metadata(root),
        manifest: serde_json::from_slice(
            &std::fs::read(root.join("benches/measurement_manifest.json"))
                .context("promise registry: missing project measurement manifest")?,
        )?,
    };
    let mut errors = Vec::new();
    for promise in review_scenarios() {
        errors.extend(
            promise
                .benchmark_keys
                .iter()
                .filter(|key| !inventory.benchmark_budgeted(key))
                .map(|key| {
                    format!(
                        "{}: {key} is absent or has no authoritative budget",
                        promise.id
                    )
                }),
        );
        for (kind, ids) in [
            ("capacity", promise.capacity_scenarios),
            ("resource", promise.resource_scenarios),
        ] {
            errors.extend(
                inventory
                    .probe_errors(kind, ids)
                    .into_iter()
                    .map(|error| format!("{}: {error}", promise.id)),
            );
        }
    }
    ensure!(
        errors.is_empty(),
        "unsatisfiable promise registry:\n{}",
        errors.join("\n")
    );
    Ok(())
}

#[cfg(test)]
mod registry_tests {
    #[test]
    fn valid_inventory_passes_but_missing_measurement_or_zero_budget_fails() {
        use serde_json::json;
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("benches")).unwrap();
        std::fs::create_dir_all(root.path().join("src/bin")).unwrap();
        let manifest_path = root.path().join("benches/measurement_manifest.json");
        assert!(super::validate(root.path()).is_err());
        std::fs::write(&manifest_path, "{}").unwrap();
        let error = super::validate(root.path()).unwrap_err().to_string();
        assert!(error.contains("session_persist_latency"));
        assert!(error.contains("large_file_first_visible_ceiling"));
        std::fs::write(root.path().join("src/bin/probe.rs"), "fn main() {}").unwrap();
        for name in ["capacity_probe", "resource_probe"] {
            std::fs::write(
                root.path().join(format!("src/bin/{name}.rs")),
                "fn main() {}",
            )
            .unwrap();
        }

        let mut manifest = json!({"probe_measurements":{},"probes":{"capacity_probe":[],"resource_probe":[]},
            "capacity_budgets":{},"resource_budgets":{}});
        for scenario in super::review_scenarios() {
            for key in scenario.benchmark_keys {
                manifest["probe_measurements"][*key] = json!({"probe":"probe","threshold_ms":1});
            }
            for (kind, ids) in [
                ("capacity", scenario.capacity_scenarios),
                ("resource", scenario.resource_scenarios),
            ] {
                let probe = format!("{kind}_probe");
                let budgets = format!("{kind}_budgets");
                for id in ids {
                    manifest["probes"][&probe]
                        .as_array_mut()
                        .unwrap()
                        .push(json!(id));
                    manifest[&budgets][*id] = json!({"threshold_ms":1,"max":1});
                }
            }
        }
        let path = root.path().join("benches/measurement_manifest.json");
        std::fs::write(&path, manifest.to_string()).unwrap();
        super::validate(root.path()).unwrap();
        manifest["probe_measurements"]["session_persist_latency"]["threshold_ms"] = json!(0);
        std::fs::write(&path, manifest.to_string()).unwrap();
        assert!(super::validate(root.path()).is_err());
    }
}

#[cfg(test)]
mod frozen_membership {
    #[test]
    fn promise_membership_and_scale_are_frozen() {
        use serde_json::json;
        let actual: Vec<_> = super::review_scenarios().iter().map(|p| json!({
            "id":p.id, "benchmark_keys":p.benchmark_keys,
            "capacity_scenarios":p.capacity_scenarios, "resource_scenarios":p.resource_scenarios,
            "scale_targets":p.scale_targets.iter().map(|t| json!({
                "id":t.id,"minimum":t.minimum,"unit":t.unit,
                "capacity_scenarios":t.capacity_scenarios,"resource_scenarios":t.resource_scenarios
            })).collect::<Vec<_>>()
        })).collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/promise-membership.json");
        let expected: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(json!(actual), expected, "promise membership or scale drift");
    }
}
