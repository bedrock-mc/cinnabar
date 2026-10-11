use std::{fs, path::Path, process::Command};

use architecture::check_repository;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// A nested worktree under an ignored path never reaches the gate; an untracked new file still does.
#[test]
fn git_ignored_trees_are_not_scanned() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers=['crates/screens']\n",
    );
    write(
        root,
        "crates/screens/Cargo.toml",
        "[package]\nname='screens'\nversion='0.1.0'\n",
    );
    write(root, "crates/screens/src/lib.rs", "pub fn ok() {}\n");
    write(
        root,
        "policy.toml",
        "production_rust_max = 1000\nmodule_root_max = 300\npowershell_max = 800\ntest_max = 1200\n[[crates]]\nname = \"screens\"\npath = \"crates/screens\"\n",
    );
    write(root, ".gitignore", "/.claude/worktrees/\n");
    let oversized = "fn f() {}\n".repeat(1001);
    write(
        root,
        ".claude/worktrees/agent/crates/screens/src/big.rs",
        &oversized,
    );
    git(root, &["init", "-q"]);

    let diagnostics = check_repository(root, &root.join("policy.toml")).unwrap();
    assert!(
        diagnostics
            .iter()
            .all(|line| !line.contains(".claude/worktrees")),
        "ignored worktree scanned: {diagnostics:?}"
    );

    write(root, "crates/screens/src/new.rs", &oversized);
    let diagnostics = check_repository(root, &root.join("policy.toml")).unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("crates/screens/src/new.rs")),
        "untracked source skipped: {diagnostics:?}"
    );
}
