use std::{fs, path::Path, process::Command};

use architecture::check_repository;

/// Creates a small crate with one external dependency and an optional policy extension.
fn fixture(root: &Path, source: &str, extra: &str) {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='sample'\nversion='0.1.0'\n[dependencies]\nother='1'\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), source).unwrap();
    fs::write(root.join("policy.toml"), format!("production_rust_max=1000\nmodule_root_max=300\npowershell_max=800\ntest_max=1200\n[[crates]]\nname='sample'\npath='.'\n{extra}")).unwrap();
}

/// Returns only forwarding and binary findings from a fixture repository.
fn findings(root: &Path) -> Vec<String> {
    check_repository(root, &root.join("policy.toml"))
        .unwrap()
        .into_iter()
        .filter(|line| line.contains("re-export") || line.contains("binary artifact"))
        .collect()
}

#[test]
fn rejects_named_renamed_grouped_and_restricted_forwarders() {
    for source in [
        "pub use other::Thing;",
        "pub(crate) use other::{Thing as Renamed};",
        "pub(super) use other::Thing;",
        "pub use other;",
        "use other as alias; pub use alias::Thing;",
        "mod other {} pub use ::other::Thing;",
        "use other::Thing; pub use self::Thing as Forwarded;",
        "mod facade { use other::Thing; pub use self::Thing as Forwarded; }",
        "mod facade { pub use other::Thing; } pub use facade::Thing;",
        "#[cfg(feature = \"optional\")] pub use other::Thing;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(!findings(temp.path()).is_empty(), "missed {source}");
    }
}

#[test]
fn rejects_multiline_and_nested_restricted_globs() {
    for source in [
        "pub(crate) use self::{nested::{*}};",
        "pub(in crate) use self::nested::\n*;",
        "pub use self::nested::*;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(
            findings(temp.path())
                .iter()
                .any(|line| line.contains("glob re-export"))
        );
    }
}

#[test]
fn permits_local_exports_and_private_dependency_imports() {
    let temp = tempfile::tempdir().unwrap();
    fixture(
        temp.path(),
        "mod other { pub struct Thing; } pub use other::Thing; mod nested { use other::Thing; use super::*; }",
        "",
    );
    assert!(findings(temp.path()).is_empty());
}

#[test]
fn follows_a_private_import_in_another_module() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "mod facade; pub use facade::Thing;", "");
    fs::write(temp.path().join("src/facade.rs"), "use other::Thing;").unwrap();
    assert!(
        findings(temp.path())
            .iter()
            .any(|line| line.contains("facade::Thing"))
    );
}

#[test]
fn follows_an_explicit_module_file_path() {
    let temp = tempfile::tempdir().unwrap();
    fixture(
        temp.path(),
        "#[path = \"renamed.rs\"] mod facade; pub use facade::Thing;",
        "",
    );
    fs::write(temp.path().join("src/renamed.rs"), "use other::Thing;").unwrap();
    assert!(
        findings(temp.path())
            .iter()
            .any(|line| line.contains("facade::Thing"))
    );
}

#[test]
fn allowances_cover_only_the_named_export_in_the_named_file() {
    let temp = tempfile::tempdir().unwrap();
    fixture(
        temp.path(),
        "pub use other::{Kept, New}; mod nested;",
        "\n[[reexport_allowances]]\npath='src/lib.rs'\nexports=['other::Kept']\n",
    );
    fs::write(temp.path().join("src/nested.rs"), "pub use other::Kept;").unwrap();
    let diagnostics = findings(temp.path());
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().any(|line| line.contains("other::New")));
    assert!(
        diagnostics
            .iter()
            .any(|line| line.starts_with("src/nested.rs"))
    );
}

#[test]
fn rejects_executable_headers_without_extensions() {
    for header in [
        b"\x7fELF".as_slice(),
        b"MZ",
        b"\xcf\xfa\xed\xfe",
        b"\xfe\xed\xfa\xce",
        b"\xca\xfe\xba\xbe",
        b"\xbf\xba\xfe\xca",
        b"\0asm",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), "", "");
        fs::write(temp.path().join("tool"), header).unwrap();
        assert!(
            findings(temp.path())
                .iter()
                .any(|line| line.contains("executable header"))
        );
    }
}

#[test]
fn permits_text_scripts_and_short_nonexecutable_files() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "", "");
    fs::write(temp.path().join("tool"), "#!/bin/sh\necho hello\n").unwrap();
    fs::write(temp.path().join("empty"), "").unwrap();
    fs::write(temp.path().join("short"), b"\x7fEL").unwrap();
    assert!(findings(temp.path()).is_empty());
}

#[test]
fn honors_binary_attributes_and_owned_art_without_allowing_executables() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(
        root,
        "",
        "\n[[owned_artifacts]]\npath='art/'\nownership_record='NOTICES.md'\n",
    );
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        root.join(".gitattributes"),
        "payload binary\nart/* binary\n",
    )
    .unwrap();
    fs::write(root.join("payload"), "arbitrary data").unwrap();
    fs::create_dir(root.join("art")).unwrap();
    fs::write(root.join("art/font"), "font bytes").unwrap();
    fs::write(root.join("art/program"), b"\x7fELF").unwrap();
    fs::write(root.join("NOTICES.md"), "art/: original artwork").unwrap();
    let diagnostics = findings(root);
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("payload: forbidden binary artifact (Git binary attribute)"))
    );
    assert!(diagnostics.iter().any(|line| line.contains("art/program: forbidden binary artifact (executable header)")));
}
