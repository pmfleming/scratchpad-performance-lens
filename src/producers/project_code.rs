mod classifier;
#[cfg(test)]
mod history_tests;

use super::common::output_text;
use super::render::render_project_code;
use crate::cli::MeasureOptions;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{bail, Result};
use classifier::{classify_path, rust_test_line_mask};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
pub fn project_code_metrics(config: &LensConfig, _options: MeasureOptions) -> Result<()> {
    match project_code_payload(config) {
        Ok(payload) => shared::write_visibility(
            &config.output_dir.join("project_code_metrics.json"),
            &payload,
            "project code metrics",
            render_project_code(&payload),
        ),
        Err(error) => {
            let payload = json!({
                "version": 1,
                "source": "rust_git_first_parent_history",
                "meta": {"probe_status": "failed", "error": error.to_string()},
                "current": null,
                "history": [],
            });
            shared::write_visibility(
                &config.output_dir.join("project_code_metrics.json"),
                &payload,
                "project code metrics",
                render_project_code(&payload),
            )?;
            bail!("project code probe failed: {error}");
        }
    }
}

fn project_code_payload(config: &LensConfig) -> Result<Value> {
    let ref_name = remote_ref(&config.project_root);
    let latest_sha = git(&config.project_root, &["rev-parse", &ref_name])?
        .trim()
        .to_string();
    let latest_subject = git(
        &config.project_root,
        &["log", "-1", "--pretty=%s", &ref_name],
    )?
    .trim()
    .to_string();
    let latest_date = git(
        &config.project_root,
        &["log", "-1", "--date=iso-strict", "--pretty=%cI", &ref_name],
    )?
    .trim()
    .to_string();
    let mut cache = HashMap::new();
    let current = count_ref(&config.project_root, &ref_name, &mut cache)?;
    let history = commit_history(&config.project_root, &ref_name, 40, &mut cache)?;
    let payload = json!({
        "version": 1,
        "source": "rust_git_first_parent_history",
        "ref": ref_name,
        "snapshot_scope": "upstream_tracking_ref_or_HEAD_not_working_tree",
        "counting_method": "exact_nonblank_rust_lines_with_embedded_test_classification",
        "timestamp_semantics": "commit_timestamp_not_push_time",
        "latest_push": {
            "sha": latest_sha,
            "short_sha": latest_sha.chars().take(8).collect::<String>(),
            "date": latest_date,
            "subject": latest_subject,
        },
        "current": current.to_json(),
        "history": history,
    });
    Ok(payload)
}

#[derive(Clone, Copy, Default)]
struct CodeCounts {
    application: i64,
    test: i64,
    other: i64,
}

impl CodeCounts {
    fn zero() -> Self {
        Self {
            application: 0,
            test: 0,
            other: 0,
        }
    }

    fn total(self) -> i64 {
        self.application + self.test + self.other
    }

    fn to_json(self) -> Value {
        json!({"application": self.application, "test": self.test, "other": self.other, "total": self.total()})
    }
}

fn git(project_root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project_root)
        .output()?;
    if !output.status.success() {
        bail!("{}", output_text(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn git_ok(project_root: &Path, args: &[&str]) -> Option<String> {
    git(project_root, args).ok()
}

fn remote_ref(project_root: &Path) -> String {
    git_ok(
        project_root,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| "HEAD".to_string())
}

fn is_included_code_path(path: &str) -> bool {
    let excluded = [
        ".git",
        ".venv",
        "target",
        "target_test",
        "target-codex",
        "assets",
        "docs",
        "fonts",
        "log",
    ];
    Path::new(path)
        .extension()
        .is_some_and(|extension| extension == "rs")
        && !path.split(['/', '\\']).any(|part| excluded.contains(&part))
}

fn count_content(path: &str, content: &str) -> CodeCounts {
    let category = classify_path(path);
    let mask = if category == "application" {
        rust_test_line_mask(content)
    } else {
        Vec::new()
    };
    let mut counts = CodeCounts::zero();
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if mask.get(index).copied().unwrap_or(false) || category == "test" {
            counts.test += 1;
        } else if category == "application" {
            counts.application += 1;
        } else {
            counts.other += 1;
        }
    }
    counts
}

type CountCache = HashMap<(String, String), CodeCounts>;

fn count_ref(project_root: &Path, ref_name: &str, cache: &mut CountCache) -> Result<CodeCounts> {
    let tree = git(project_root, &["ls-tree", "-r", "-z", ref_name])?;
    let mut totals = CodeCounts::zero();
    for record in tree.split('\0').filter(|record| !record.is_empty()) {
        let (header, path) = record.split_once('\t').ok_or_else(|| anyhow::anyhow!("invalid git tree record"))?;
        if !is_included_code_path(path) { continue; }
        let blob = header.split_whitespace().last().ok_or_else(|| anyhow::anyhow!("missing git blob id"))?;
        let key = (blob.to_owned(), classify_path(path).to_owned());
        let counts = if let Some(counts) = cache.get(&key) { *counts } else {
            // Cache by content and path category: renames across src/tests must reclassify.
            let content = git(project_root, &["cat-file", "blob", blob])?;
            let counts = count_content(path, &content);
            cache.insert(key, counts);
            counts
        };
        totals.application += counts.application;
        totals.test += counts.test;
        totals.other += counts.other;
    }
    Ok(totals)
}

fn commit_history(
    project_root: &Path,
    ref_name: &str,
    limit: usize,
    cache: &mut CountCache,
) -> Result<Vec<Value>> {
    let output = git(
        project_root,
        &[
            "log",
            "--first-parent",
            &format!("--max-count={limit}"),
            "--date=iso-strict",
            "--pretty=format:%H%x09%cI%x09%s",
            ref_name,
        ],
    )?;
    let commits: Vec<_> = output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some((
                parts.next()?.to_string(),
                parts.next()?.to_string(),
                parts.next()?.to_string(),
            ))
        })
        .collect();
    commits.into_iter().rev().map(|(sha, date, subject)| {
        let counts = count_ref(project_root, &sha, cache)?;
        Ok(json!({"sha": sha, "short_sha": sha.chars().take(8).collect::<String>(), "date": date, "subject": subject, "lines": counts.to_json()}))
    }).collect()
}
