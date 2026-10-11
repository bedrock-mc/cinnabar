use std::{fs, process::Command};

#[test]
fn build_directory_environment_expands_workspace_templates() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/lib.rs"), "").unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='env-template-fixture'\nversion='0.1.0'\nedition='2024'\n[workspace]\n",
    )
    .unwrap();
    let output = Command::new("cargo")
        .current_dir(temp.path())
        .env("CARGO_BUILD_BUILD_DIR", "{workspace-root}/target/ra-build")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        metadata["build_directory"].as_str().unwrap(),
        std::fs::canonicalize(temp.path())
            .unwrap()
            .join("target/ra-build")
            .to_str()
            .unwrap()
    );
}
