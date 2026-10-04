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
fn gameplay_and_session_accept_their_separate_domain_dependencies() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(
        root,
        "gameplay",
        "[dependencies]\nclient-world={path='../client-world'}\ninventory={path='../inventory'}\nsim={path='../sim'}\nprotocol={path='../protocol'}",
    );
    set_dependencies(
        root,
        "client-session",
        "[dependencies]\nprotocol={path='../protocol'}\nresource-pack={path='../resource-pack'}\npack-compiler={path='../pack-compiler'}\nassets={path='../assets'}",
    );
    assert_eq!(diagnostics(root), Vec::<String>::new());
}

#[test]
fn domains_reject_app_engine_presentation_and_each_other_in_every_dependency_kind() {
    for name in ["gameplay", "client-session"] {
        let peer = if name == "gameplay" {
            "client-session"
        } else {
            "gameplay"
        };
        for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
            for prefix in ["", "target.'cfg(windows)'."] {
                for package in [
                    "bedrock-client",
                    "client-ui",
                    "client-presentation",
                    "render",
                    "bevy_ecs",
                    "wgpu_core",
                    "chunk-pipeline",
                    peer,
                ] {
                    let temp = fixture();
                    set_dependencies(
                        temp.path(),
                        name,
                        &format!("[{prefix}{kind}]\nrenamed={{package='{package}',version='1'}}"),
                    );
                    assert!(
                        diagnostics(temp.path()).iter().any(|line| line
                            == &format!("{name}: forbidden dependency path `{name} -> {package}`")),
                        "missed {name} {prefix}{kind} {package}"
                    );
                }
            }
        }
    }
}

#[test]
fn domains_reject_hidden_renderer_back_edges() {
    for name in ["gameplay", "client-session"] {
        let temp = fixture();
        set_dependencies(
            temp.path(),
            name,
            "[dependencies]\nassets={path='../assets'}",
        );
        set_dependencies(
            temp.path(),
            "assets",
            "[target.'cfg(windows)'.build-dependencies]\ngpu={package='bevy_render',version='1'}",
        );
        assert!(diagnostics(temp.path()).iter().any(|line| line
            == &format!("{name}: forbidden dependency path `{name} -> assets -> bevy_render`")));
    }
}

#[test]
fn domains_reject_rooted_and_renamed_app_adapters() {
    for name in ["gameplay", "client-session"] {
        for source in [
            "use crate::runtime::NetworkHandle;",
            "use crate::{camera as view, ui_runtime::UiRuntime};",
            "pub use super::super::acceptance as evidence;",
            "fn run(_: crate::presentation::Frame) {}",
            "#[cfg(any(test, feature = \"extra\"))] use crate::audio::Sound;",
        ] {
            let temp = fixture();
            write(
                &temp
                    .path()
                    .join(format!("crates/{name}/src/domain/adapter.rs")),
                source,
            );
            assert!(
                diagnostics(temp.path())
                    .iter()
                    .any(|line| line.contains("crosses forbidden")),
                "missed {name}: {source}"
            );
        }
    }
}

#[test]
fn domains_borrow_authority_without_owning_a_second_copy() {
    for name in ["gameplay", "client-session"] {
        let temp = fixture();
        let file = temp.path().join(format!("crates/{name}/src/state.rs"));
        write(
            &file,
            "struct Frame<'a> { inventory: &'a inventory::PlayerInventoryLedger } #[cfg(test)] struct Fixture { store: world::ChunkStore }",
        );
        assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
        for source in [
            "use world::ChunkStore as Store; struct State { copy: Option<Store> }",
            "type Ledger = inventory::PlayerInventoryLedger; struct State { copy: Box<Ledger> }",
            "struct State { authority: client_world::WorldAuthority }",
        ] {
            write(&file, source);
            assert!(
                diagnostics(temp.path())
                    .iter()
                    .any(|line| line.contains("owns forbidden authority")),
                "missed {name}: {source}"
            );
        }
    }
}

#[test]
fn domain_fixtures_cannot_be_enabled_by_production_dependencies() {
    for name in ["gameplay", "client-session"] {
        let temp = fixture();
        set_dependencies(
            temp.path(),
            name,
            "[features]\nfixtures=['test-support']\ntest-support=[]",
        );
        write(
            &temp.path().join("app/Cargo.toml"),
            &format!(
                "[package]\nname='bedrock-client'\nversion='0.1.0'\n[dependencies]\ndomain={{package='{name}',path='../crates/{name}',features=['fixtures']}}\n"
            ),
        );
        assert!(diagnostics(temp.path()).iter().any(|line| line
            == &format!(
                "app: production dependency `{name}` enables test-support feature `test-support`"
            )));
    }
}
