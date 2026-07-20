use std::{fs, io::Write as _, path::Path};

use architecture::check_repository;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(path, contents).expect("write fixture");
}

fn fixture_policy(root: &Path) -> std::path::PathBuf {
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers=['crates/alpha','crates/beta']\n",
    );
    let policy = root.join("policy.toml");
    write(
        &policy,
        r#"
production_rust_max = 5
module_root_max = 4
powershell_max = 5
test_max = 6
forbidden_artifacts = ["**/*.exe", "**/*.png"]

[[vendored]]
path = "vendor/"
ownership_record = "vendor/UPSTREAM.md"

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
        "pub use hidden::*;\npub fn exposed_for_test() {}\npub fn for_test() {}\nuse crate::*;\ninclude!(\"fragment.rs\");\n",
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
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers=['crates/alpha','crates/beta','crates/gamma']\n",
    );

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
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("workspace member `crates/gamma` has no crate rule"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("missing ownership record"))
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
            .any(|line| line.contains("crate-wide private preludes"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("source fragments"))
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
fn resolves_inherited_workspace_dependencies_before_enforcing_edges() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    let policy = fixture_policy(root);
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers=['crates/alpha','crates/beta']\n[workspace.dependencies]\nrenamed={package='beta',path='crates/beta'}\n",
    );
    write(
        &root.join("crates/alpha/Cargo.toml"),
        "[package]\nname='alpha'\nversion='0.1.0'\n[target.'cfg(windows)'.build-dependencies]\nrenamed.workspace=true\n",
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
    write(
        &root.join("vendor/UPSTREAM.md"),
        "owned upstream snapshot\n",
    );

    let diagnostics = check_repository(root, &policy).expect("run checker");
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("local dependency `beta` is absent from the allowlist"))
    );
}

#[test]
fn permits_declared_vendor_and_log_only_markers() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    let policy = fixture_policy(root);
    write(
        &root.join("vendor/UPSTREAM.md"),
        "owned upstream snapshot\n",
    );
    write(&root.join("vendor/generated.rs"), &"line\n".repeat(50));
    write(&root.join(".local/captured.png"), "ignored local artifact");
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

#[test]
fn grandfathered_line_baseline_allows_only_the_recorded_size() {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    let policy = fixture_policy(root);
    fs::OpenOptions::new()
        .append(true)
        .open(&policy)
        .expect("open fixture policy")
        .write_all(b"\n[[line_baselines]]\npath = 'crates/alpha/tests/large.rs'\nmax = 8\n")
        .expect("append line baseline");
    write(
        &root.join("crates/alpha/Cargo.toml"),
        "[package]\nname='alpha'\nversion='0.1.0'\n",
    );
    write(
        &root.join("crates/beta/Cargo.toml"),
        "[package]\nname='beta'\nversion='0.1.0'\n",
    );
    write(
        &root.join("crates/alpha/tests/large.rs"),
        &"line\n".repeat(8),
    );
    write(
        &root.join("app/src/acceptance/markers.rs"),
        "const A: &str = \"RUST_MCBE_READY\";\n",
    );
    write(
        &root.join("scripts/acceptance/Markers.ps1"),
        "$marker = 'RUST_MCBE_READY'\n",
    );
    write(
        &root.join("vendor/UPSTREAM.md"),
        "owned upstream snapshot\n",
    );

    assert_eq!(
        check_repository(root, &policy).expect("run checker at baseline"),
        Vec::<String>::new()
    );
    write(
        &root.join("crates/alpha/tests/large.rs"),
        &"line\n".repeat(9),
    );
    let diagnostics = check_repository(root, &policy).expect("run checker above baseline");
    assert!(diagnostics.iter().any(|line| {
        line.contains("crates/alpha/tests/large.rs: 9 lines exceeds grandfathered baseline 8")
    }));
}
