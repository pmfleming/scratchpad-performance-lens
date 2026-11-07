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
        without_descriptions(&mut actual);
        assert_eq!(&actual, expected,
            "{file}: gate drift requires an explicit reviewed inventory revision, manifest version bump and rationale");
    }
}
