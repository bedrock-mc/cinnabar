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
        "pub use r#other::Thing;",
        "pub use other::Thing as other;",
        "use other::Thing as other; pub use other::Member;",
        "use r#other as alias; pub use alias::Thing;",
        "pub(crate) use other::{Thing as Renamed};",
        "pub(super) use other::Thing;",
        "pub use other;",
        "pub fn other() {} pub use other as api;",
        "mod local { pub fn other() {} } use local::*; pub use other as api;",
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
    fixture(temp.path(), "mod r#facade; pub use facade::Thing;", "");
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
fn conditional_module_paths_keep_local_exports_in_their_declaring_crate() {
    for attribute in [
        r#"#[cfg_attr(feature = "alternate", path = "alternate.rs")]"#,
        r#"#[cfg_attr(feature = "outer", cfg_attr(feature = "inner", path = "alternate.rs"))]"#,
        r#"#[r#cfg_attr(feature = "alternate", r#path = "alternate.rs")]"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(
            temp.path(),
            &format!("pub struct Thing; {attribute} mod facade; pub use facade::Thing as Public;"),
            "",
        );
        for file in ["facade.rs", "alternate.rs"] {
            fs::write(temp.path().join("src").join(file), "pub use super::Thing;").unwrap();
        }
        assert!(findings(temp.path()).is_empty(), "{attribute}");
    }
}

#[test]
fn conditional_module_paths_check_only_the_selected_module_contents() {
    let temp = tempfile::tempdir().unwrap();
    fixture(
        temp.path(),
        r#"#[cfg_attr(feature = "alternate", path = "alternate.rs")] mod facade;
        pub use facade::Thing;"#,
        "",
    );
    fs::write(temp.path().join("src/facade.rs"), "pub struct Thing;").unwrap();
    let alternate = temp.path().join("src/alternate.rs");
    fs::write(
        &alternate,
        r#"#[cfg(not(feature = "alternate"))] pub use other::Thing;
        #[cfg(feature = "alternate")] pub struct Thing;"#,
    )
    .unwrap();
    assert!(findings(temp.path()).is_empty());
    fs::write(alternate, "use other::Thing;").unwrap();
    assert!(
        findings(temp.path())
            .iter()
            .any(|line| line.contains("facade::Thing"))
    );
}

#[test]
fn conditional_inline_module_paths_select_the_child_directory() {
    let temp = tempfile::tempdir().unwrap();
    fixture(
        temp.path(),
        r#"pub struct Thing;
        #[cfg_attr(feature = "alternate", path = "alternate")]
        mod inline { mod facade; pub use facade::Thing as Public; }"#,
        "",
    );
    for directory in ["inline", "alternate"] {
        let directory = temp.path().join("src").join(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("facade.rs"), "pub use super::super::Thing;").unwrap();
    }
    assert!(findings(temp.path()).is_empty());
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

#[test]
fn follows_private_globs_and_rejects_uncertain_namespace_ownership() {
    for source in [
        "use other::*; pub use Thing;",
        "use other as alias; use alias::*; pub(crate) use Thing;",
        "mod facade { use other::*; pub use Thing; } pub use facade::Thing;",
        "mod facade { pub use other::Thing; } use facade::*; pub use Thing;",
        "use std::fmt::*; pub fn Debug() {} pub use self::Debug as Exported;",
        "mod local { pub fn Debug() {} } use local::Debug; use std::fmt::*; pub use Debug as Exported;",
        "use std::mem::*; pub struct drop {} pub use self::drop as Exported;",
        "use std::fmt::*; pub fn local_function() {} pub use self::local_function as Exported;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(!findings(temp.path()).is_empty(), "missed {source}");
    }
    let temp = tempfile::tempdir().unwrap();
    fixture(
        temp.path(),
        "use other::Unrelated; pub struct Thing; pub use self::Thing as Local;",
        "",
    );
    assert!(findings(temp.path()).is_empty());
}

#[test]
fn keeps_library_binary_and_integration_target_imports_independent() {
    for (library, binary) in [
        (
            "use other::Thing;",
            "mod local { pub struct Thing; } use local::Thing; pub(crate) use self::Thing as Public;",
        ),
        (
            "mod local { pub struct Thing; } use local::Thing; pub(crate) use self::Thing as Public;",
            "use other::Thing;",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fixture(root, library, "");
        fs::write(root.join("src/main.rs"), binary).unwrap();
        fs::create_dir(root.join("tests")).unwrap();
        fs::write(root.join("tests/independent.rs"), "use other::Thing;").unwrap();
        assert!(findings(root).is_empty());
    }
}

#[test]
fn resolves_modules_beside_a_custom_cargo_target_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root, "use other::Thing;", "");
    let manifest = root.join("Cargo.toml");
    let mut contents = fs::read_to_string(&manifest).unwrap();
    contents.push_str("\n[[bin]]\nname='tool'\npath='custom/entry.rs'\n");
    fs::write(manifest, contents).unwrap();
    fs::create_dir(root.join("custom")).unwrap();
    fs::write(
        root.join("custom/entry.rs"),
        "mod local; pub use local::Thing;",
    )
    .unwrap();
    fs::write(root.join("custom/local.rs"), "pub use other::Thing;").unwrap();
    assert!(
        findings(root)
            .iter()
            .any(|line| line.starts_with("custom/entry.rs:"))
    );
}

#[test]
fn resolves_forwarding_types_after_expanding_local_module_aliases() {
    for exports in [
        "use facade as alias; pub use alias::Thing;",
        "use facade as alias; use alias::*; pub use Thing;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(
            temp.path(),
            &format!("mod facade {{ pub use other::Thing; }} {exports}"),
            "\n[[reexport_allowances]]\npath='src/lib.rs'\nexports=['other::Thing']\n",
        );
        assert_eq!(findings(temp.path()).len(), 1, "missed {exports}");
    }
}

#[test]
fn explicit_dependency_imports_do_not_taint_local_glob_exports() {
    for imports in [
        "use std::fmt::Debug; use local::*;",
        "use local::*; use std::fmt::Debug;",
    ] {
        for local in [
            "pub struct Thing;",
            "pub enum Items { Thing } pub use Items::Thing;",
        ] {
            let temp = tempfile::tempdir().unwrap();
            fixture(
                temp.path(),
                &format!("mod local {{ {local} }} {imports} pub use self::Thing as Exported;"),
                "",
            );
            let diagnostics = findings(temp.path());
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
        }
    }
}

#[test]
fn relative_roots_keep_parent_module_exports_local() {
    let current = std::env::current_dir().unwrap();
    let temp = tempfile::tempdir_in(&current).unwrap();
    let relative = Path::new(".").join(temp.path().strip_prefix(&current).unwrap());
    fixture(&relative, "pub struct Thing; mod nested;", "");
    fs::write(
        relative.join("src/nested.rs"),
        "pub use super::Thing as Exported;",
    )
    .unwrap();
    for root in [relative.clone(), relative.join("src/..")] {
        assert!(findings(&root).is_empty());
    }
    fs::write(relative.join("src/nested.rs"), "pub use other::Thing;").unwrap();
    let diagnostics = findings(&relative);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].starts_with("src/nested.rs:"));
}

#[test]
fn checks_every_conditional_import_alternative_regardless_of_order() {
    let external = "#[cfg(feature = \"external\")] use other::Thing;";
    let local = "#[cfg(not(feature = \"external\"))] use local::Thing;";
    for imports in [format!("{external} {local}"), format!("{local} {external}")] {
        let temp = tempfile::tempdir().unwrap();
        fixture(
            temp.path(),
            &format!(
                "mod local {{ pub struct Thing; }} {imports} pub use self::Thing as Exported;"
            ),
            "",
        );
        let diagnostics = findings(temp.path());
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].contains("other::Thing"));
    }
}

#[test]
fn conditional_local_definitions_cannot_hide_external_glob_bindings() {
    for source in [
        r#"#[cfg(feature = "local")] pub struct Thing;
            #[cfg(not(feature = "local"))] use other::*;
            pub use self::Thing as Exported;"#,
        r#"#[cfg(any(feature = "a", feature = "b"))] pub struct Thing;
            #[cfg(not(any(feature = "a", feature = "b")))] use other::*;
            pub use self::Thing as Exported;"#,
        r#"#[r#cfg(feature = "local")] pub struct Thing;
            #[cfg(not(feature = "local"))] use other::*;
            pub use self::Thing as Exported;"#,
        r#"#[cfg_attr(feature = "local", cfg(any()))] use other::*;
            #[cfg(feature = "local")] pub struct Thing;
            pub use self::Thing as Exported;"#,
        r#"#[cfg(feature = "local")] mod other { pub struct Thing; }
            #[cfg(not(feature = "local"))] pub use other::Thing;"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert_eq!(findings(temp.path()).len(), 1, "missed {source}");
    }
}

#[test]
fn local_type_qualifiers_take_precedence_over_external_globs() {
    for source in [
        "use std::fmt::*; pub enum Values { Thing } pub use self::Values::Thing;",
        "use other::*; pub enum Values { Thing } use self::Values as Alias; pub use Alias::Thing;",
        "mod local { use other::*; pub enum Values { Thing } } pub use local::Values::Thing;",
        r#"use other::*;
            #[cfg(feature = "local")] pub enum Values { Thing }
            #[cfg(feature = "local")] pub use self::Values::Thing;"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(findings(temp.path()).is_empty(), "rejected {source}");
    }
    for source in [
        "use other::*; fn Values() {} pub use self::Values::Thing;",
        r#"use other::*;
            #[cfg(feature = "local")] pub enum Values { Thing }
            pub use self::Values::Thing;"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(!findings(temp.path()).is_empty(), "missed {source}");
    }
}

#[test]
fn mutually_exclusive_external_imports_do_not_taint_local_exports() {
    for source in [
        r#"mod local { pub struct Thing; }
            #[cfg(feature = "external")] use other::Thing;
            #[cfg(not(feature = "external"))] use local::Thing;
            #[cfg(not(feature = "external"))] pub use self::Thing as Exported;"#,
        r#"use other::Unrelated;
            #[cfg(feature = "local")] pub struct Thing;
            #[cfg(not(feature = "local"))] pub struct Thing;
            pub use self::Thing as Exported;"#,
        r#"mod local { pub struct Thing; }
            #[cfg(target_os = "windows")] use other::Thing;
            #[cfg(target_os = "linux")] use local::Thing;
            #[cfg(target_os = "linux")] pub use self::Thing as Exported;"#,
        r#"#[cfg(feature = "local")] mod other { pub struct Thing; }
            #[cfg(feature = "local")] pub use other::Thing;"#,
        r#"mod local { pub struct Thing; }
            #[cfg(windows)] use other::Thing;
            #[cfg(not(windows))] use local::Thing;
            #[cfg(target_os = "linux")] pub use self::Thing as Exported;"#,
        r#"mod local { pub struct Thing; }
            #[cfg(not(unix))] use other::Thing;
            #[cfg(unix)] use local::Thing;
            #[cfg(target_family = "unix")] pub use self::Thing as Exported;"#,
        r#"mod local { pub struct Thing; }
            #[cfg(target_endian = "big")] use other::Thing;
            #[cfg(target_endian = "little")] use local::Thing;
            #[cfg(target_arch = "x86_64")] pub use self::Thing as Exported;"#,
        r#"#![cfg(any())]
            pub use other::Thing;"#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(findings(temp.path()).is_empty(), "rejected {source}");
    }
}

#[test]
fn external_module_conditions_are_inherited_by_local_definitions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(
        root,
        r#"#[cfg(feature = "local")] mod local;
        #[cfg(feature = "local")] use local::*;
        #[cfg(not(feature = "local"))] use other::*;
        pub use self::Thing as Exported;"#,
        "",
    );
    fs::write(root.join("src/local.rs"), "pub struct Thing;").unwrap();
    assert_eq!(findings(root).len(), 1);
}

#[test]
fn possible_platforms_and_build_options_keep_external_exports_visible() {
    for predicate in [
        r#"all(windows, target_os = "windows")"#,
        r#"all(unix, target_os = "linux")"#,
        r#"all(target_family = "unix", target_family = "wasm")"#,
        r#"all(target_os = "linux", panic = "abort")"#,
        r#"target_os = "custom""#,
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(
            temp.path(),
            &format!("#[cfg({predicate})] pub use other::Thing;"),
            "",
        );
        assert_eq!(findings(temp.path()).len(), 1, "missed {predicate}");
    }
}

#[test]
fn recognizes_implicit_default_and_renamed_library_dependencies() {
    for (library, extra, file) in [
        ("sample", "", "src/lib.rs"),
        (
            "renamed_api",
            "\n[lib]\nname='renamed_api'\npath='src/library.rs'\n",
            "src/library.rs",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fixture(root, "pub struct Thing;", "");
        let manifest = root.join("Cargo.toml");
        let contents = fs::read_to_string(&manifest).unwrap() + extra;
        fs::write(manifest, contents).unwrap();
        fs::write(root.join(file), "pub struct Thing;").unwrap();
        for target in [
            "src/main.rs",
            "tests/forward.rs",
            "benches/forward.rs",
            "examples/forward.rs",
        ] {
            let target = root.join(target);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, format!("pub use {library}::Thing;")).unwrap();
        }
        assert_eq!(findings(root).len(), 4);
    }
}

#[test]
fn value_definitions_do_not_shadow_dependency_type_paths() {
    for definition in [
        "fn other() {}",
        "fn helper() {} use helper as other;",
        "mod local { pub fn helper() {} } use local::helper as other;",
        "const other: u8 = 0;",
        "static other: u8 = 0;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(
            temp.path(),
            &format!("{definition} pub use other::Thing;"),
            "",
        );
        assert_eq!(findings(temp.path()).len(), 1, "missed {definition}");
    }
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "pub struct other; pub use other as Local;", "");
    assert!(findings(temp.path()).is_empty());
}

#[test]
fn local_glob_modules_shadow_dependency_names() {
    for source in [
        "mod local { pub mod other { pub struct Thing; } } use local::*; pub use other::Thing;",
        "mod local { pub mod other { pub struct Thing; } } mod nested { use crate::local::*; pub use other::Thing; }",
        "mod local { pub mod other { pub struct Thing; } } use std::fmt::Debug; use local::*; pub use other::Thing;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(findings(temp.path()).is_empty(), "rejected {source}");
    }
    for source in [
        "mod local { pub fn other() {} } use local::*; pub use other::Thing;",
        "mod local { pub mod other { pub use ::other::Thing; } } use local::*; pub use other::Thing;",
        "use other::*; pub use other::Thing;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(!findings(temp.path()).is_empty(), "missed {source}");
    }
}

#[test]
fn self_crate_aliases_keep_exports_local() {
    for source in [
        "pub extern crate self as api; pub struct Thing; pub use api::Thing as Local;",
        "extern crate self as api; pub struct Thing; pub use ::api::Thing as Local;",
        "extern crate self as api; pub struct Thing; mod nested { pub use ::api::Thing as Local; }",
        "extern crate self as api; pub struct Thing; mod nested { pub use api::Thing as Local; }",
        "extern crate other as api; mod nested { mod api { pub struct Thing; } pub use api::Thing; }",
        "pub struct Thing; mod nested { pub extern crate self as api; pub use api::Thing as Local; }",
        "extern crate self as other; pub struct Thing; pub use other::Thing as Local;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(findings(temp.path()).is_empty(), "rejected {source}");
    }
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), "pub extern crate other as api;", "");
    assert_eq!(findings(temp.path()).len(), 1);
}

#[test]
fn inaccessible_glob_items_do_not_hide_external_exports() {
    for source in [
        "mod local { struct Arc; } use local::*; use std::sync::*; pub use Arc as Exported;",
        "mod local { pub(self) struct Arc; } use local::*; use std::sync::*; pub use Arc as Exported;",
        "mod owned { pub struct Arc; } mod local { use crate::owned::Arc; } use local::*; use std::sync::*; pub use Arc as Exported;",
        "mod owned { pub struct Arc; } mod local { use crate::owned::*; } use local::*; use std::sync::*; pub use Arc as Exported;",
        "mod parent { use std::sync::*; mod child { use super::*; pub use Arc as Exported; } }",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(!findings(temp.path()).is_empty(), "missed {source}");
    }
}

#[test]
fn accessible_local_glob_items_keep_their_owner() {
    for source in [
        "mod local { pub(super) struct Arc; } use local::*; use std::fmt::Debug; pub(crate) use Arc as Exported;",
        "mod local { pub(in crate) struct Arc; } use local::*; use std::fmt::Debug; pub(crate) use Arc as Exported;",
        "mod local { pub enum Values { Arc } } use local::Values::*; use std::fmt::Debug; pub use Arc as Exported;",
        "mod owned { pub struct Arc; } mod local { pub use crate::owned::Arc; } use local as alias; use alias::*; use std::fmt::Debug; pub use Arc as Exported;",
        "mod parent { struct Arc; mod child { use super::*; pub(super) use Arc as Exported; } }",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert!(findings(temp.path()).is_empty(), "rejected {source}");
    }
}

#[test]
fn external_crate_aliases_respect_configuration_and_absolute_paths() {
    for source in [
        "extern crate other; pub use ::other::Thing;",
        "extern crate other as api; pub use ::api::Thing;",
        "extern crate other as api; mod nested { pub use api::Thing; }",
        "mod other {} extern crate other as api; pub use api::Thing;",
        "#[cfg(any())] extern crate self as other; pub use ::other::Thing;",
    ] {
        let temp = tempfile::tempdir().unwrap();
        fixture(temp.path(), source, "");
        assert_eq!(findings(temp.path()).len(), 1, "missed {source}");
    }
}
