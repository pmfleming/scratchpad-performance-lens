//! Project-owned exact and minimum fixture requirements, shared by diagnostics and reviews.
use serde_json::Value;

pub fn matches(details: &Value, contract: &Value) -> bool {
    let Some(equals) = contract["equals"].as_object().filter(|v| !v.is_empty()) else {
        return false;
    };
    equals
        .iter()
        .all(|(key, expected)| &details[key] == expected)
        && contract["minimum"]
            .as_object()
            .into_iter()
            .flat_map(|values| values.iter())
            .all(|(key, minimum)| {
                details[key]
                    .as_u64()
                    .zip(minimum.as_u64())
                    .is_some_and(|(actual, limit)| actual >= limit)
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn revision_and_retention_cannot_be_replaced_by_a_large_workload_label() {
        let contract = json!({"equals":{"fixture_revision":2,"history_retention":"unbounded"},
            "minimum":{"history_payload_bytes":52428800}});
        let mut sample = json!({"fixture_revision":1,"history_retention":"unbounded","history_payload_bytes":52428800});
        assert!(!matches(&sample, &contract));
        sample["fixture_revision"] = json!(2);
        assert!(matches(&sample, &contract));
        sample["history_payload_bytes"] = json!(16777216);
        assert!(!matches(&sample, &contract));
    }
}
