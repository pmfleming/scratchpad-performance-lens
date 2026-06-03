mod classifier;

use super::common::output_text;
use super::render::render_project_code;
use crate::config::LensConfig;
use crate::shared;
use anyhow::{bail, Result};
use classifier::{classify_path, rust_test_line_mask};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
pub fn project_code_metrics(config: &LensConfig) -> Result<()> {
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
    let current = count_ref(&config.project_root, &ref_name)?;
    let history = commit_history(&config.project_root, &ref_name, 40, current)?;
    let payload = json!({
        "version": 1,
        "source": "rust_git_first_parent_history",
        "ref": ref_name,
        "latest_push": {
            "sha": latest_sha,
            "short_sha": latest_sha.chars().take(8).collect::<String>(),
            "date": latest_date,
            "subject": latest_subject,
        },
        "current": current.to_json(),
        "history": history,
    });
    shared::write_visibility(
        &config.output_dir.join("project_code_metrics.json"),
        &payload,
        "project code metrics",
        render_project_code(&payload),
    )
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

fn tracked_code_paths(project_root: &Path, ref_name: &str) -> Result<Vec<String>> {
    Ok(
        git(project_root, &["ls-tree", "-r", "--name-only", ref_name])?
            .lines()
            .map(str::trim)
            .filter(|path| !path.is_empty() && is_included_code_path(path))
            .map(ToString::to_string)
            .collect(),
    )
}

fn read_file_at_ref(project_root: &Path, ref_name: &str, path: &str) -> Option<String> {
    git(project_root, &["show", &format!("{ref_name}:{path}")]).ok()
}

fn count_path(project_root: &Path, ref_name: &str, path: &str) -> CodeCounts {
    let Some(content) = read_file_at_ref(project_root, ref_name, path) else {
        return CodeCounts::zero();
    };
    let category = classify_path(path);
    let mask = if category == "application" {
        rust_test_line_mask(&content)
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

fn count_ref(project_root: &Path, ref_name: &str) -> Result<CodeCounts> {
    let mut totals = CodeCounts::zero();
    for path in tracked_code_paths(project_root, ref_name)? {
        let counts = count_path(project_root, ref_name, &path);
        totals.application += counts.application;
        totals.test += counts.test;
        totals.other += counts.other;
    }
    Ok(totals)
}

fn commit_delta(project_root: &Path, sha: &str) -> CodeCounts {
    let mut totals = CodeCounts::zero();
    let Some(output) = git_ok(
        project_root,
        &["show", "--first-parent", "--format=", "--numstat", sha],
    ) else {
        return totals;
    };
    for line in output.lines() {
        let parts: Vec<_> = line.split('\t').collect();
        if parts.len() < 3 || !is_included_code_path(parts[2]) {
            continue;
        }
        let (Ok(added), Ok(deleted)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) else {
            continue;
        };
        let net = added - deleted;
        match classify_path(parts[2]) {
            "application" => totals.application += net,
            "test" => totals.test += net,
            _ => totals.other += net,
        }
    }
    totals
}

fn subtract_counts(value: CodeCounts, delta: CodeCounts) -> CodeCounts {
    CodeCounts {
        application: (value.application - delta.application).max(0),
        test: (value.test - delta.test).max(0),
        other: (value.other - delta.other).max(0),
    }
}

fn commit_history(
    project_root: &Path,
    ref_name: &str,
    limit: usize,
    current: CodeCounts,
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
    let mut counts_by_sha = HashMap::new();
    let mut running = current;
    for (sha, _, _) in &commits {
        counts_by_sha.insert(sha.clone(), running);
        running = subtract_counts(running, commit_delta(project_root, sha));
    }
    Ok(commits.into_iter().rev().map(|(sha, date, subject)| {
        let counts = counts_by_sha.get(&sha).copied().unwrap_or_else(CodeCounts::zero);
        json!({"sha": sha, "short_sha": sha.chars().take(8).collect::<String>(), "date": date, "subject": subject, "lines": counts.to_json()})
    }).collect())
}
