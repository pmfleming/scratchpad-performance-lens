use crate::config::LensConfig;
use crate::shared;
use serde_json::Value;

pub(in crate::producers) fn read_analysis(
    config: &LensConfig,
    file: &str,
    default: Value,
) -> Value {
    let path = config.output_dir.join(file);
    let status = crate::provenance::validity(&path, &crate::provenance::context(&config.project_root));
    if status != "valid" {
        eprintln!("Excluding {file}: {status} measurement provenance");
        return default;
    }
    shared::read_json(&path).into_value_or(default)
}

pub(in crate::producers) fn array_value(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

pub(in crate::producers) fn values_array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub(in crate::producers) fn scenarios_array(value: &Value) -> &[Value] {
    value
        .get("scenarios")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
