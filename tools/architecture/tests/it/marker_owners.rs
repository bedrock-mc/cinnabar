use std::{fs, path::Path};

use architecture::check_repository;

/// Writes one fixture source after creating its parent directories.
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

/// Creates an extracted crate that owns one symbolic environment marker.
fn fixture() -> tempfile::TempDir {
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
    write(
        root,
        "policy.toml",
        r#"
production_rust_max = 1000
module_root_max = 300
powershell_max = 800
test_max = 1200
[[crates]]
name = "screens"
path = "crates/screens"
[[markers]]
literal = "RUST_MCBE_READY"
kind = "environment_variable"
producer = "crates/screens/src/diagnostic_markers.rs"
consumer = "crates/screens/src/observer.rs"
"#,
    );
    write(
        root,
        "crates/screens/src/diagnostic_markers.rs",
        "pub const READY: &str = \"RUST_MCBE_READY\";\n",
    );
    write(
        root,
        "crates/screens/src/observer.rs",
        "fn observe() { let _ = std::env::var(crate::diagnostic_markers::READY); }\n",
    );
    temp
}

#[test]
fn extracted_owner_preserves_the_symbolic_marker_contract() {
    let temp = fixture();
    assert_eq!(
        check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn extracted_owner_rejects_undeclared_markers_elsewhere_in_its_source_tree() {
    let temp = fixture();
    write(
        temp.path(),
        "crates/screens/src/other.rs",
        "const OTHER: &str = \"RUST_MCBE_UNDECLARED\";\n",
    );
    let diagnostics = check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|line| line == "marker `RUST_MCBE_UNDECLARED` has no declared expectation")
    );
}

#[test]
fn extracted_owner_requires_the_declared_consumer_to_use_the_symbol() {
    let temp = fixture();
    write(
        temp.path(),
        "crates/screens/src/observer.rs",
        "fn observe() {}\n",
    );
    let diagnostics = check_repository(temp.path(), &temp.path().join("policy.toml")).unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("has no symbolic use in declared consumer"))
    );
}
