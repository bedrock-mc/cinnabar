use std::{fs, path::Path};

use architecture::check_repository;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(path, contents).expect("write fixture");
}

fn fixture_policy(root: &Path) -> std::path::PathBuf {
    let policy = root.join("policy.toml");
    write(
        &policy,
        r#"
production_rust_max = 5
module_root_max = 4
powershell_max = 5
test_max = 6
vendored_paths = ["vendor/"]
forbidden_artifacts = ["**/*.exe", "**/*.png"]

[[crates]]
name = "alpha"
path = "crates/alpha"
allowed_dependencies = []
forbidden_dependencies = ["bevy", "wgpu"]

[[crates]]
name = "beta"
path = "crates/beta"
allowed_dependencies = []

[[markers]]
literal = "RUST_MCBE_READY"
kind = "parsed"
producer = "app/src/acceptance/markers.rs"
consumer = "scripts/acceptance/Markers.ps1"
"#,
    );
    policy
}

#[test]
fn rejects_each_structural_policy_violation_with_sorted_diagnostics() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    let policy = fixture_policy(root);
    write(
        &root.join("crates/alpha/Cargo.toml"),
        "[package]\nname='alpha'\nversion='0.1.0'\n[build-dependencies]\nbevy='1'\n[target.'cfg(windows)'.dependencies]\nrenamed={package='beta',path='../beta'}\n[dev-dependencies]\nwgpu='1'\n",
    );
    write(
        &root.join("crates/beta/Cargo.toml"),
        "[package]\nname='beta'\nversion='0.1.0'\n",
    );
    write(
        &root.join("crates/alpha/src/lib.rs"),
        "pub use hidden::*;\npub fn exposed_for_test() {}\n\n\n\n",
    );
    write(
        &root.join("app/src/acceptance/markers.rs"),
        "const A: &str = \"RUST_MCBE_READY\";\nconst B: &str = \"RUST_MCBE_READY\";\n",
    );
    write(
        &root.join("scripts/acceptance/Markers.ps1"),
        "$marker = 'RUST_MCBE_READY'\n",
    );
    write(&root.join("app/captured.png"), "not really an image");

    let diagnostics = check_repository(root, &policy).expect("run checker");
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("forbidden dependency `bevy`"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("local dependency `beta` is absent from the allowlist"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("matches forbidden artifact pattern"))
    );
    assert!(!diagnostics.iter().any(|line| line.contains("wgpu")));
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("glob re-export"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("test-only public API"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("exceeds module-root limit"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("producer count 2"))
    );
    assert!(diagnostics.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn permits_declared_vendor_and_log_only_markers() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    let policy = fixture_policy(root);
    write(&root.join("vendor/generated.rs"), &"line\n".repeat(50));
    write(
        &root.join("crates/alpha/Cargo.toml"),
        "[package]\nname='alpha'\nversion='0.1.0'\n",
    );
    write(
        &root.join("crates/beta/Cargo.toml"),
        "[package]\nname='beta'\nversion='0.1.0'\n",
    );
    write(&root.join("crates/alpha/src/lib.rs"), "pub fn api() {}\n");
    write(
        &root.join("app/src/acceptance/markers.rs"),
        "const A: &str = \"RUST_MCBE_READY\";\n",
    );
    write(
        &root.join("scripts/acceptance/Markers.ps1"),
        "$marker = 'RUST_MCBE_READY'\n",
    );

    assert_eq!(
        check_repository(root, &policy).expect("run checker"),
        Vec::<String>::new()
    );
}
