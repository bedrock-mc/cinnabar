use std::{fs, path::Path};

use architecture::check_repository;

/// Write a fixture file after creating its parent directory.
fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    fs::write(path, contents).expect("write fixture");
}

/// Build empty packages using the repository's real dependency policy.
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("fixture root");
    let root = temp.path();
    let mut policy: toml::Value =
        toml::from_str(include_str!("../policy.toml")).expect("repository policy");
    policy
        .as_table_mut()
        .expect("policy table")
        .retain(|key, _| {
            matches!(
                key,
                "crates"
                    | "production_rust_max"
                    | "module_root_max"
                    | "powershell_max"
                    | "test_max"
            )
        });
    let mut members = Vec::new();
    for rule in policy["crates"].as_array().expect("crate rules") {
        let path = rule["path"].as_str().expect("crate path");
        let name = rule["name"].as_str().expect("crate name");
        members.push(format!("'{path}'"));
        write(
            &root.join(path).join("Cargo.toml"),
            &format!("[package]\nname='{name}'\nversion='0.1.0'\n"),
        );
    }
    write(
        &root.join("policy.toml"),
        &toml::to_string(&policy).expect("serialize fixture policy"),
    );
    write(
        &root.join("Cargo.toml"),
        &format!("[workspace]\nmembers=[{}]\n", members.join(",")),
    );
    temp
}

/// Replace one crate manifest with the requested dependency declarations.
fn set_dependencies(root: &Path, name: &str, dependencies: &str) {
    write(
        &root.join("crates").join(name).join("Cargo.toml"),
        &format!("[package]\nname='{name}'\nversion='0.1.0'\n{dependencies}\n"),
    );
}

/// Return all diagnostics from the fixture's copy of the real policy.
fn diagnostics(root: &Path) -> Vec<String> {
    check_repository(root, &root.join("policy.toml")).expect("check fixture")
}

#[test]
fn permits_shared_contract_without_game_state_in_render() {
    let temp = fixture();
    let root = temp.path();
    for name in ["render", "chunk-pipeline", "protocol"] {
        set_dependencies(
            root,
            name,
            "[dependencies]\nrender-api={path='../render-api'}",
        );
    }
    assert_eq!(diagnostics(root), Vec::<String>::new());
}

#[test]
fn contract_rejects_local_and_external_dependencies_of_every_kind() {
    for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
        for prefix in ["", "target.'cfg(windows)'."] {
            for dependency in ["world={path='../world'}", "serde='1'"] {
                let temp = fixture();
                let root = temp.path();
                set_dependencies(
                    root,
                    "render-api",
                    &format!("[{prefix}{kind}]\n{dependency}"),
                );
                assert!(
                    diagnostics(root)
                        .iter()
                        .any(|line| line.starts_with("render-api: dependency-free crate")),
                    "missed {prefix}{kind}: {dependency}",
                );
            }
        }
    }
}

#[test]
fn render_rejects_game_state_including_aliases_and_test_dependencies() {
    for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
        for prefix in ["", "target.'cfg(windows)'."] {
            for name in ["client-world", "protocol"] {
                let temp = fixture();
                let root = temp.path();
                set_dependencies(
                    root,
                    "render",
                    &format!("[{prefix}{kind}]\nrenamed={{package='{name}',path='../{name}'}}"),
                );
                assert!(diagnostics(root).iter().any(|line| {
                    line == &format!("render: forbidden dependency path `render -> {name}`")
                }));
            }
        }
    }
}

#[test]
fn render_rejects_game_state_through_inherited_transitive_build_dependencies() {
    let temp = fixture();
    let root = temp.path();
    let workspace = fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    write(
        &root.join("Cargo.toml"),
        &format!(
            "{workspace}\n[workspace.dependencies]\nstate={{package='protocol',path='crates/protocol'}}\n",
        ),
    );
    set_dependencies(
        root,
        "render",
        "[dependencies]\nmeshing={path='../meshing'}",
    );
    set_dependencies(root, "meshing", "[dependencies]\nworld={path='../world'}");
    set_dependencies(
        root,
        "world",
        "[target.'cfg(windows)'.build-dependencies]\nstate.workspace=true",
    );
    assert!(diagnostics(root).iter().any(|line| {
        line == "render: forbidden dependency path `render -> meshing -> world -> protocol`"
    }));
}

#[test]
fn render_rejects_local_dependencies_without_rules() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(root, "render", "[dependencies]\nhelper={path='../helper'}");
    assert!(diagnostics(root).iter().any(|line| {
        line == "render: dependency path `render -> helper` has no crate rule; cannot verify boundary"
    }));
}

#[test]
fn render_rejects_renamed_game_state_without_a_local_path() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(
        root,
        "render",
        "[dependencies]\nstate={package='protocol',version='1'}",
    );
    assert!(
        diagnostics(root)
            .iter()
            .any(|line| { line == "render: forbidden dependency path `render -> protocol`" })
    );
}

#[test]
fn dependency_tests_do_not_become_transitive_build_edges() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(
        root,
        "render",
        "[dependencies]\nmeshing={path='../meshing'}",
    );
    set_dependencies(
        root,
        "meshing",
        "[dev-dependencies]\nprotocol={path='../protocol'}",
    );
    assert_eq!(diagnostics(root), Vec::<String>::new());
}
