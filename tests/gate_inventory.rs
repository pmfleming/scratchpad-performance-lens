//! Freeze project-owned gates independently of newly added descriptions.
use serde_json::Value;
use std::path::PathBuf;

fn without_descriptions(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.remove("outcome");
            for child in fields.values_mut() {
                without_descriptions(child);
            }
        }
        Value::Array(children) => children.iter_mut().for_each(without_descriptions),
        _ => {}
    }
}

#[test]
fn project_gates_match_cycle25_inventory() {
    let root = std::env::var_os("SCRATCHPAD_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scratchpad"));
    let frozen: Value = serde_json::from_str(include_str!("fixtures/gates-cycle25.json")).unwrap();
    for (file, expected) in frozen.as_object().unwrap() {
        let path = root.join("benches").join(file);
        let mut actual: Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap_or_else(|error| {
                panic!("{}: {error}; set SCRATCHPAD_PROJECT_ROOT", path.display())
            }))
            .unwrap();
        // New policy diagnostics do not replace the original gate fields.
        actual.as_object_mut().unwrap().remove("durability_policy");
        actual
            .as_object_mut()
            .unwrap()
            .remove("outcome_contract_version");
        if let Some(d) = actual.get_mut("diagnostics").and_then(Value::as_object_mut) {
            d.remove("residency");
        }
        without_descriptions(&mut actual);
        assert_eq!(&actual, expected,
            "{file}: gate drift requires an explicit reviewed inventory revision, manifest version bump and rationale");
    }
}

#[test]
fn outcome_endpoints_and_policy_match_versioned_inventory() {
    let root = std::env::var_os("SCRATCHPAD_PROJECT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scratchpad"));
    let manifest = scratchpad_performance_lens::outcomes::load(&root).unwrap();
    let expected: Value = serde_json::from_str(include_str!("fixtures/outcomes-v1.json")).unwrap();
    let mut actual = serde_json::json!({
        "outcome_contract_version":manifest["outcome_contract_version"],
        "durability_policy":manifest["durability_policy"],
        "residency_diagnostics":manifest["diagnostics"]["residency"]
    });
    for section in scratchpad_performance_lens::outcomes::SECTIONS {
        actual[section] = manifest[section]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, entry)| (id.clone(), entry["outcome"].clone()))
            .collect();
    }
    assert_eq!(
        actual, expected,
        "Outcome or policy drift requires a reviewed contract version and inventory revision"
    );
    assert!(
        !scratchpad_performance_lens::residency::analyze(&manifest, Vec::new(), &[])
            .unwrap()
            .complete
    );
}
