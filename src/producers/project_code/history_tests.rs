use super::{commit_history, count_ref, git};
use std::collections::HashMap;
use std::path::Path;

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]).unwrap();
    git(root, &["-c", "user.name=fixture", "-c", "user.email=fixture@example.test", "-c", "commit.gpgsign=false", "commit", "-m", message]).unwrap();
}

#[test]
fn history_counts_each_revision_consistently_across_blanks_tests_renames_and_merges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "--initial-branch=main"]).unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    let original = "pub fn app() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn works() {}\n}\n";
    std::fs::write(root.join("src/lib.rs"), original).unwrap();
    commit(root, "initial");
    let initial = git(root, &["rev-parse", "HEAD"]).unwrap().trim().to_owned();
    std::fs::write(root.join("src/lib.rs"), format!("\n\n{original}\n\n")).unwrap();
    commit(root, "blank lines");
    std::fs::write(root.join("src/lib.rs"), "pub fn app() {}\n").unwrap();
    commit(root, "remove embedded tests");
    std::fs::create_dir(root.join("tests")).unwrap();
    std::fs::rename(root.join("src/lib.rs"), root.join("tests/renamed file.rs")).unwrap();
    commit(root, "rename across categories");
    git(root, &["checkout", "-b", "side", &initial]).unwrap();
    std::fs::write(root.join("src/extra.rs"), "pub fn extra() {}\n").unwrap();
    commit(root, "side branch");
    git(root, &["checkout", "main"]).unwrap();
    git(root, &["-c", "user.name=fixture", "-c", "user.email=fixture@example.test", "-c", "commit.gpgsign=false",
        "merge", "--no-ff", "side", "-m", "merge side"]).unwrap();
    let mut cache = HashMap::new();
    let history = commit_history(root, "HEAD", 40, &mut cache).unwrap();
    let categories: Vec<_> = history.iter().map(|row| (row["lines"]["application"].as_i64().unwrap(), row["lines"]["test"].as_i64().unwrap())).collect();
    assert_eq!(categories, [(1,5),(1,5),(1,0),(0,1),(1,1)]);
    assert_eq!(history.last().unwrap()["lines"], count_ref(root,"HEAD",&mut cache).unwrap().to_json());
}
