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
