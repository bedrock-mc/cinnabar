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
        toml::from_str(include_str!("../../policy.toml")).expect("repository policy");
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
                    | "module_boundaries"
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
fn presentation_accepts_borrowed_observations_and_shared_asset_contracts() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-presentation",
        "[dependencies]\nassets={path='../assets'}\nclient-world={package='chunk-pipeline',path='../chunk-pipeline'}\nrender={path='../render'}",
    );
    write(
        &temp
            .path()
            .join("crates/client-presentation/src/adapter.rs"),
        "struct Observation<'a> { stream: Option<&'a mut client_world::WorldStream>, state: &'a sim::PlayerState }\nfn observe(_: render::ActorRenderScene) {}",
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn plugins_reject_host_gameplay_session_and_reverse_evidence_edges() {
    for owner in ["client-presentation", "acceptance", "diagnostics"] {
        for package in ["bedrock-client", "gameplay", "client-session"] {
            for kind in [
                "dependencies",
                "dev-dependencies",
                "target.'cfg(windows)'.build-dependencies",
            ] {
                let temp = fixture();
                set_dependencies(
                    temp.path(),
                    owner,
                    &format!("[{kind}]\nalias={{package='{package}',version='1'}}"),
                );
                assert!(
                    diagnostics(temp.path())
                        .iter()
                        .any(|line| line.contains("forbidden dependency path")),
                    "{owner} -> {package}"
                );
            }
        }
    }
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-presentation",
        "[dependencies]\nacceptance={path='../acceptance'}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("client-presentation -> acceptance"))
    );
}

#[test]
fn plugins_reject_app_modules_and_authority_ownership() {
    for owner in ["client-presentation", "acceptance"] {
        for source in [
            "use crate::{runtime as host}; fn read(_: host::World) {}",
            "use crate::movement::State;",
            "struct Owner { stream: client_world::WorldStream }",
            "struct Owner { player: player_state::PlayerState }",
            "struct Owner { network: NetworkHandle }",
        ] {
            let temp = fixture();
            write(
                &temp.path().join(format!("crates/{owner}/src/adapter.rs")),
                source,
            );
            assert!(
                diagnostics(temp.path())
                    .iter()
                    .any(|line| line.contains("forbidden")),
                "missed {owner}: {source}"
            );
        }
    }
}

#[test]
fn presentation_cannot_hide_session_dependency_behind_shared_crate() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "client-presentation",
        "[dependencies]\nclient-ui={path='../client-ui'}",
    );
    set_dependencies(
        temp.path(),
        "client-ui",
        "[dependencies]\ntransport={package='client-session',version='1'}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("client-presentation -> client-ui -> client-session"))
    );
}

#[test]
fn presentation_test_support_cannot_leak_into_production() {
    let temp = fixture();
    write(
        &temp.path().join("app/Cargo.toml"),
        "[package]\nname='bedrock-client'\nversion='0.1.0'\n[dependencies]\nclient-presentation={path='../crates/client-presentation',features=['test-support']}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("test-support"))
    );
}
