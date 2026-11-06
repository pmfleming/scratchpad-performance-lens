use crate::config::LensConfig;
use crate::{provenance, shared};
use serde_json::Value;

/// A report's inputs share one project snapshot, not one repository hash per file.
pub(in crate::producers) struct AnalysisInputs<'a> {
    config: &'a LensConfig,
    pub context: Value,
}

impl<'a> AnalysisInputs<'a> {
    pub fn new(config: &'a LensConfig) -> Self {
        Self {
            config,
            context: provenance::context(&config.project_root),
        }
    }

    pub fn read(&self, file: &str, default: Value) -> Value {
        let path = self.config.output_dir.join(file);
        let status = provenance::validity(&path, &self.context);
        if status != "valid" {
            eprintln!("Excluding {file}: {status} measurement provenance");
            return default;
        }
        shared::read_json(&path).into_value_or(default)
    }
}

pub(in crate::producers) fn values_array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}

pub(in crate::producers) fn scenarios_array(value: &Value) -> &[Value] {
    values_array(&value["scenarios"])
}
