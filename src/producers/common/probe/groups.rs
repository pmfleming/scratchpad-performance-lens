//! Only project-declared fixture families may share a probe process.
use anyhow::{ensure, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) struct Group<'a> {
    pub id: Option<&'a str>,
    pub scenarios: Vec<&'a str>,
}

pub(super) fn select<'a>(
    manifest: &'a Value,
    bin: &str,
    selected: &[&'a str],
) -> Result<Vec<Group<'a>>> {
    let mut declared = Vec::new();
    let mut members = BTreeSet::new();
    let mut names = BTreeSet::new();
    #[derive(Deserialize)]
    struct Declaration<'a> {
        id: &'a str,
        scenarios: Vec<&'a str>,
    }
    let groups = manifest["probe_fixture_groups"]
        .get(bin)
        .map(Vec::<Declaration<'_>>::deserialize)
        .transpose()?
        .unwrap_or_default();
    for group in groups {
        let id = group.id;
        ensure!(
            !id.is_empty() && names.insert(id),
            "empty or duplicate fixture group: {id}"
        );
        ensure!(
            group.scenarios.len() >= 2,
            "fixture group must declare at least two scenarios"
        );
        for scenario in &group.scenarios {
            ensure!(
                manifest["probes"][bin]
                    .as_array()
                    .is_some_and(|known| known.iter().any(|value| value == scenario)),
                "unknown fixture group scenario: {scenario}"
            );
            ensure!(
                members.insert(*scenario),
                "overlapping fixture group scenario: {scenario}"
            );
        }
        let scenarios: Vec<_> = group
            .scenarios
            .into_iter()
            .filter(|id| selected.contains(id))
            .collect();
        if !scenarios.is_empty() {
            declared.push(Group {
                id: Some(id),
                scenarios,
            });
        }
    }
    let mut result = Vec::new();
    for &scenario in selected {
        if result
            .iter()
            .any(|group: &Group<'_>| group.scenarios.contains(&scenario))
        {
            continue;
        }
        if let Some(index) = declared
            .iter()
            .position(|group| group.scenarios.contains(&scenario))
        {
            result.push(declared.remove(index));
        } else {
            result.push(Group {
                id: None,
                scenarios: vec![scenario],
            });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::select;
    use serde_json::json;
    #[test]
    fn only_explicit_disjoint_known_groups_share_selected_work() {
        let mut manifest = json!({"probes":{"resource_probe":["a","b","c","d"]},
            "probe_fixture_groups":{"resource_probe":[{"id":"shared","scenarios":["a","c","b"]}]}});
        let groups = select(&manifest, "resource_probe", &["b", "d", "a"]).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].scenarios, ["a", "b"]);
        assert_eq!(groups[1].scenarios, ["d"]);
        assert!(groups[1].id.is_none());
        assert_eq!(
            select(&manifest, "resource_probe", &["c"]).unwrap()[0].scenarios,
            ["c"]
        );
        manifest["probe_fixture_groups"]["resource_probe"][0]["scenarios"][2] = json!("a");
        assert!(select(&manifest, "resource_probe", &["a"]).is_err());
        manifest["probe_fixture_groups"]["resource_probe"][0]["scenarios"][2] = json!("unknown");
        assert!(select(&manifest, "resource_probe", &["a"]).is_err());
        assert_eq!(
            select(&json!({}), "resource_probe", &["a", "b"])
                .unwrap()
                .len(),
            2
        );
    }
}
