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
fn movement_rejects_direct_grouped_renamed_and_reexported_ui_paths() {
    for source in [
        "use crate::ui_runtime::UiRuntime;",
        "use crate::{ui_runtime::{UiRuntime as State}, runtime::WorldClock};",
        "pub use super::super::ui_runtime as screens;",
        "fn sample(_: &ui::HudStore) {}",
        "use crate::UiRuntime as Screens; fn sample(_: &Screens) {}",
        "#[cfg(any(test, feature = \"extra\"))] use crate::ui_runtime::UiRuntime;",
    ] {
        let temp = fixture();
        write(&temp.path().join("app/src/movement/local_facts.rs"), source);
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("crosses forbidden")),
            "missed {source}"
        );
    }
}

#[test]
fn comments_strings_and_test_only_items_are_not_production_dependencies() {
    for source in [
        "// use crate::ui_runtime::UiRuntime;",
        r#"const NOTE: &str = "ui::HudStore and ui_runtime::UiRuntime";"#,
        "#[cfg(test)] mod tests { use crate::ui_runtime::UiRuntime; }",
        r#"#[cfg(all(test, feature = "extra"))] use ui::HudStore;"#,
        "struct Movement { #[cfg(test)] ui: ui::HudStore }",
        "struct Movement; impl Movement { #[cfg(test)] fn test_helper(_: &ui::HudStore) {} }",
    ] {
        let temp = fixture();
        write(&temp.path().join("app/src/movement.rs"), source);
        write(
            &temp.path().join("app/src/movement/facts_tests.rs"),
            "use crate::ui_runtime::UiRuntime;",
        );
        assert_eq!(
            diagnostics(temp.path()),
            Vec::<String>::new(),
            "false positive for {source}"
        );
    }
}

#[test]
fn ui_cannot_retain_authority_through_aliases_generics_or_enum_payloads() {
    for source in [
        "struct UiRuntime { ledger: inventory::PlayerInventoryLedger }",
        "use inventory::{InventorySession as State}; struct UiRuntime { state: State }",
        "use client_world::LocalPlayerFacts as Facts; type State = Option<Box<Facts>>; struct UiRuntime { state: State }",
        "struct Holder { evidence: Option<protocol::AbilitiesUpdate> }",
        "enum Holder { State(crate::player_runtime::PlayerRuntime) }",
        "struct Holder { state: player_state::PlayerState }",
        "type State = inventory::InventorySession; struct UiRuntime { state: State }",
    ] {
        let temp = fixture();
        for path in [
            "app/src/ui_runtime.rs",
            "crates/client-ui/src/ui_runtime.rs",
        ] {
            write(&temp.path().join(path), source);
        }
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("owns forbidden authority")),
            "missed {source}"
        );
    }
}

#[test]
fn ui_can_borrow_authority_and_hold_a_captured_ledger_but_not_own_authority() {
    let temp = fixture();
    write(
        &temp.path().join("app/src/ui_runtime.rs"),
        r#"
        struct Context<'a> {
            facts: &'a client_world::LocalPlayerFacts,
            inventory: &'a mut inventory::InventorySession,
            resource: bevy::prelude::ResMut<'a, crate::player_runtime::PlayerRuntime>,
        }
        #[cfg(test)] struct Fixture { state: inventory::InventorySession }
    "#,
    );
    let snapshot = temp
        .path()
        .join("crates/client-ui/src/ui_runtime/presentation_snapshot.rs");
    write(
        &snapshot,
        "struct PresentationInventory { ledger: player_state::CapturedLedger }",
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());

    for source in [
        "struct UiRuntime { ledger: inventory::PlayerInventoryLedger }",
        "struct PresentationInventory { ledger: inventory::PlayerInventoryLedger }",
        "struct PresentationInventory { facts: client_world::LocalPlayerFacts }",
        "struct PresentationInventory { session: inventory::InventorySession }",
    ] {
        write(&snapshot, source);
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("owns forbidden authority")),
            "missed {source}"
        );
    }
}

#[test]
fn a_broken_module_cannot_silently_skip_boundary_checks() {
    let temp = fixture();
    write(
        &temp.path().join("app/src/ui_runtime.rs"),
        "struct UiRuntime {",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("cannot parse module boundary"))
    );
}

#[test]
fn inventory_rejects_ui_engine_and_game_state_in_every_dependency_kind() {
    for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
        for prefix in ["", "target.'cfg(windows)'."] {
            for package in [
                "bevy",
                "bevy_ecs",
                "wgpu_core",
                "ui",
                "json-ui",
                "render",
                "client-world",
                "world",
            ] {
                let temp = fixture();
                set_dependencies(
                    temp.path(),
                    "inventory",
                    &format!("[{prefix}{kind}]\nalias={{package='{package}',version='1'}}"),
                );
                assert!(diagnostics(temp.path()).iter().any(|line| line
                    == &format!("inventory: forbidden dependency path `inventory -> {package}`")));
            }
        }
    }
}

#[test]
fn inventory_and_facts_reject_ui_hidden_behind_the_protocol_crate() {
    let temp = fixture();
    for name in ["inventory", "client-world"] {
        set_dependencies(
            temp.path(),
            name,
            "[dependencies]\nprotocol={path='../protocol'}",
        );
    }
    set_dependencies(temp.path(), "protocol", "[dependencies]\nui={path='../ui'}");
    for name in ["inventory", "client-world"] {
        assert!(
            diagnostics(temp.path()).iter().any(|line| line
                == &format!("{name}: forbidden dependency path `{name} -> protocol -> ui`"))
        );
    }
}

#[test]
fn declared_vendor_manifests_are_followed_instead_of_hiding_transitive_edges() {
    let temp = fixture();
    let root = temp.path();
    let policy = fs::read_to_string(root.join("policy.toml")).unwrap();
    write(
        &root.join("policy.toml"),
        &format!("{policy}\n[[vendored]]\npath='vendor/'\nownership_record='vendor/UPSTREAM.md'\n"),
    );
    write(&root.join("vendor/UPSTREAM.md"), "Fixture vendor");
    write(
        &root.join("vendor/wire/Cargo.toml"),
        "[package]\nname='wire'\nversion='1'\n[dependencies]\nbevy='1'\n",
    );
    set_dependencies(
        root,
        "inventory",
        "[dependencies]\nprotocol={path='../protocol'}",
    );
    set_dependencies(
        root,
        "protocol",
        "[dependencies]\nwire={path='../../vendor/wire'}",
    );
    assert!(diagnostics(root).iter().any(|line| line
        == "inventory: forbidden dependency path `inventory -> protocol -> wire -> bevy`"));
}

#[test]
fn external_modules_inherit_test_cfg_but_any_production_import_remains_checked() {
    let temp = fixture();
    let root = temp.path();
    write(
        &root.join("app/src/ui_runtime.rs"),
        "#[cfg(test)] mod fixture; #[cfg(test)] #[path = \"ui_runtime/explicit.rs\"] mod named;",
    );
    write(
        &root.join("app/src/ui_runtime/fixture.rs"),
        "struct Fixture { state: inventory::InventorySession } mod child;",
    );
    write(
        &root.join("app/src/ui_runtime/fixture/child.rs"),
        "struct Fixture { state: client_world::LocalPlayerFacts }",
    );
    write(
        &root.join("app/src/ui_runtime/explicit.rs"),
        "struct Fixture { state: inventory::InventorySession }",
    );
    assert_eq!(diagnostics(root), Vec::<String>::new());
    write(
        &root.join("app/src/ui_runtime.rs"),
        "#[cfg(test)] mod fixture; #[path = \"ui_runtime/fixture.rs\"] mod live;",
    );
    assert!(
        diagnostics(root)
            .iter()
            .any(|line| line.starts_with("app/src/ui_runtime/fixture.rs:")
                && line.contains("owns forbidden authority"))
    );
}

#[test]
fn authority_aliases_cannot_hide_in_another_production_module() {
    for source in [
        "mod state; struct UiRuntime { state: state::State }",
        "mod state; use state::State as Hidden; struct UiRuntime { state: Hidden }",
        "mod state; mod holder { use super::state::State as Hidden; struct Holder { state: Option<Hidden> } }",
    ] {
        let temp = fixture();
        write(&temp.path().join("app/src/ui_runtime.rs"), source);
        write(
            &temp.path().join("app/src/ui_runtime/state.rs"),
            "pub type State = inventory::InventorySession;",
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("owns forbidden authority")),
            "missed {source}"
        );
    }
}

#[test]
fn crate_relative_authority_aliases_do_not_collide_between_packages() {
    for authority_path in ["app/src", "crates/ui/src"] {
        let temp = fixture();
        let root = temp.path();
        let policy = fs::read_to_string(root.join("policy.toml")).unwrap();
        write(
            &root.join("policy.toml"),
            &format!(
                "{policy}\n[[module_boundaries]]\npath='crates/ui/src'\nforbidden_owned_types=['InventorySession']\n"
            ),
        );
        for path in ["app/src", "crates/ui/src"] {
            write(
                &root.join(path).join("ui_runtime.rs"),
                "mod state; use state::State as Hidden; struct UiRuntime { state: Hidden }",
            );
            write(
                &root.join(path).join("ui_runtime/state.rs"),
                if path == authority_path {
                    "pub type State = inventory::InventorySession;"
                } else {
                    "pub type State = u64;"
                },
            );
        }
        let violations: Vec<_> = diagnostics(root)
            .into_iter()
            .filter(|line| line.contains("owns forbidden authority"))
            .collect();
        assert_eq!(violations.len(), 1, "{authority_path}: {violations:?}");
        assert!(violations[0].starts_with(&format!("{authority_path}/ui_runtime.rs:")));
    }
}

#[test]
fn a_production_module_cannot_escape_checks_by_using_a_test_filename() {
    let temp = fixture();
    write(
        &temp.path().join("app/src/ui_runtime.rs"),
        "mod hidden_tests;",
    );
    write(
        &temp.path().join("app/src/ui_runtime/hidden_tests.rs"),
        "struct Holder { state: inventory::InventorySession }",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.contains("owns forbidden authority"))
    );
}

#[test]
fn vendor_dependency_cycles_terminate_without_hiding_forbidden_packages() {
    let temp = fixture();
    let root = temp.path();
    let policy = fs::read_to_string(root.join("policy.toml")).unwrap();
    write(
        &root.join("policy.toml"),
        &format!("{policy}\n[[vendored]]\npath='vendor/'\nownership_record='vendor/UPSTREAM.md'\n"),
    );
    write(&root.join("vendor/UPSTREAM.md"), "Fixture vendor");
    write(
        &root.join("vendor/wire/Cargo.toml"),
        "[package]\nname='wire'\nversion='1'\n[dependencies]\nprotocol={path='../../crates/protocol'}\nbevy_ecs='1'\n",
    );
    set_dependencies(
        root,
        "inventory",
        "[dependencies]\nprotocol={path='../protocol'}",
    );
    set_dependencies(
        root,
        "protocol",
        "[dependencies]\nwire={path='../../vendor/wire'}",
    );
    assert!(diagnostics(root).iter().any(|line| line
        == "inventory: forbidden dependency path `inventory -> protocol -> wire -> bevy_ecs`"));
}

#[test]
fn production_module_remapping_cannot_hide_authority_aliases() {
    for attribute in [
        "#[path = \"ui_runtime/model.rs\"]",
        "#[cfg_attr(feature = \"alternate\", path = \"ui_runtime/model.rs\")]",
        "#[cfg_attr(feature = \"outer\", cfg_attr(feature = \"inner\", path = \"ui_runtime/model.rs\"))]",
    ] {
        let temp = fixture();
        write(
            &temp.path().join("app/src/ui_runtime.rs"),
            &format!("{attribute} mod state; struct UiRuntime {{ state: state::Alias }}"),
        );
        write(
            &temp.path().join("app/src/ui_runtime/model.rs"),
            "pub type Alias = inventory::InventorySession;",
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("forbidden #[path] remapping")),
            "missed {attribute}"
        );
    }
}

#[test]
fn production_console_satisfies_module_boundary_policy() {
    let temp = fixture();
    write(
        &temp.path().join("crates/diagnostics/src/console.rs"),
        include_str!("../../../../crates/diagnostics/src/console.rs"),
    );
    write(
        &temp
            .path()
            .join("crates/diagnostics/src/console/raw_stderr.rs"),
        include_str!("../../../../crates/diagnostics/src/console/raw_stderr.rs"),
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn test_only_module_remapping_remains_allowed() {
    let temp = fixture();
    write(
        &temp.path().join("app/src/ui_runtime.rs"),
        "#[cfg(test)] #[path = \"ui_runtime/model.rs\"] mod state;",
    );
    write(
        &temp.path().join("app/src/ui_runtime/model.rs"),
        "struct Fixture { state: inventory::InventorySession }",
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn nested_src_directory_does_not_change_the_owning_crate_alias_scope() {
    let temp = fixture();
    write(
        &temp.path().join("app/src/ui_runtime.rs"),
        "mod src; struct UiRuntime { state: src::state::Alias }",
    );
    write(
        &temp.path().join("app/src/ui_runtime/src.rs"),
        "pub mod state;",
    );
    write(
        &temp.path().join("app/src/ui_runtime/src/state.rs"),
        "pub type Alias = inventory::InventorySession;",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| line.starts_with("app/src/ui_runtime.rs:")
                && line.contains("owns forbidden authority"))
    );
}
