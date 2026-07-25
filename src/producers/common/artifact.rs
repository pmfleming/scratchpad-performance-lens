use crate::config::LensConfig;
use crate::shared;
use serde_json::Value;

pub(in crate::producers) fn read_analysis(
    config: &LensConfig,
    file: &str,
    default: Value,
) -> Value {
    shared::read_json(&config.output_dir.join(file)).into_value_or(default)
}

pub(in crate::producers) fn array_value(value: &Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

pub(in crate::producers) fn scenarios_array(value: &Value) -> &[Value] {
    value
        .get("scenarios")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
