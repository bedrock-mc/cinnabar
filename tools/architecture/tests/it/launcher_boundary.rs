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
fn new_crates_accept_the_domain_and_presentation_dependency_direction() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(
        root,
        "player-state",
        "[dependencies]\ninventory={path='../inventory'}\nclient-world={path='../client-world'}",
    );
    set_dependencies(
        root,
        "launcher",
        "[dependencies]\nui={path='../ui'}\nprotocol={path='../protocol'}",
    );
    set_dependencies(
        root,
        "client-ui",
        "[dependencies]\nlauncher={path='../launcher'}\nplayer-state={path='../player-state'}\nrender-model={path='../render-model'}\nclient-world={package='chunk-pipeline',path='../chunk-pipeline'}\n[dev-dependencies]\npack-compiler={path='../pack-compiler'}",
    );
    write(
        &root.join("app/Cargo.toml"),
        "[package]\nname='bedrock-client'\nversion='0.1.0'\n[dependencies]\nclient-ui={path='../crates/client-ui'}\nlauncher={path='../crates/launcher'}\nplayer-state={path='../crates/player-state'}\n",
    );
    assert_eq!(diagnostics(root), Vec::<String>::new());
}

/// Presentation takes render data from render-model so it never waits on the GPU renderer.
#[test]
fn client_ui_rejects_the_render_crate() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(
        root,
        "client-ui",
        "[dependencies]\nrender={path='../render'}",
    );
    let found = diagnostics(root);
    assert!(
        found
            .iter()
            .any(|line| line == "client-ui: forbidden dependency path `client-ui -> render`"),
        "{found:?}"
    );
}

#[test]
fn launcher_and_player_state_reject_engine_and_ui_dependencies_in_every_kind() {
    for name in ["launcher", "player-state"] {
        for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
            for prefix in ["", "target.'cfg(windows)'."] {
                for package in [
                    "bedrock-client",
                    "client-ui",
                    "render",
                    "bevy_ecs",
                    "wgpu_core",
                ] {
                    let temp = fixture();
                    set_dependencies(
                        temp.path(),
                        name,
                        &format!("[{prefix}{kind}]\nrenamed={{package='{package}',version='1'}}"),
                    );
                    assert!(diagnostics(temp.path()).iter().any(|line| line
                        == &format!("{name}: forbidden dependency path `{name} -> {package}`")));
                }
            }
        }
    }
}

#[test]
fn ui_rejects_app_dependencies_even_through_a_local_adapter() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(
        root,
        "client-ui",
        "[dependencies]\nlauncher={path='../launcher'}",
    );
    set_dependencies(
        root,
        "launcher",
        "[dependencies]\nhost={package='bedrock-client',path='../../app'}",
    );
    assert!(
        diagnostics(root)
            .iter()
            .any(|line| line
                == "client-ui: forbidden dependency path `client-ui -> launcher -> app`")
    );
}

#[test]
fn ui_rejects_rooted_app_modules_and_grouped_or_renamed_imports() {
    for source in [
        "use crate::runtime::NetworkHandle;",
        "use crate::{movement::Tick, camera as host_camera};",
        "pub use super::super::mining as interaction;",
        "fn read(_: crate::item_use::UseFrame) {}",
        "#[cfg(any(test, feature = \"extra\"))] use crate::acceptance::Markers;",
    ] {
        let temp = fixture();
        write(
            &temp
                .path()
                .join("crates/client-ui/src/ui_runtime/adapter.rs"),
            source,
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("crosses forbidden crate module")),
            "missed {source}"
        );
    }
}

#[test]
fn rooted_module_rules_allow_borrowed_authority_local_names_and_test_adapters() {
    let temp = fixture();
    write(
        &temp.path().join("crates/client-ui/src/ui_runtime.rs"),
        r#"
        struct Context<'a> { player: &'a mut player_state::PlayerState }
        fn draw(runtime: u64, camera: u64) -> u64 { runtime + camera }
        fn project(_: render::camera::View) {}
        #[cfg(test)] mod tests { use crate::runtime::NetworkHandle; }
    "#,
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn launcher_cannot_import_ui_presentation_or_transport_handles() {
    for source in [
        "use client_ui::UiRuntime;",
        "use crate::ui_runtime::presentation::menu_reference;",
        "use crate::presentation::Screen;",
        "struct Services { connection: NetworkHandle }",
    ] {
        let temp = fixture();
        write(&temp.path().join("crates/launcher/src/lib.rs"), source);
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line.contains("crosses forbidden")),
            "missed {source}"
        );
    }
}

#[test]
fn shared_fixtures_remain_callable_only_from_app_tests() {
    let temp = fixture();
    let root = temp.path();
    let file = root.join("app/src/adapter.rs");
    write(&file, "use client_ui::test_support::fixture;");
    assert!(
        diagnostics(root)
            .iter()
            .any(|line| line.contains("crosses forbidden `test_support`"))
    );
    write(
        &file,
        "#[cfg(test)] mod checks { use client_ui::test_support::fixture; }",
    );
    assert_eq!(diagnostics(root), Vec::<String>::new());
}

#[test]
fn explicit_dev_dependencies_can_enable_shared_fixture_features() {
    let temp = fixture();
    write(
        &temp.path().join("app/Cargo.toml"),
        r#"
        [package]
        name='bedrock-client'
        version='0.1.0'
        [dependencies]
        client-ui={path='../crates/client-ui'}
        [dev-dependencies]
        client-ui={path='../crates/client-ui',features=['test-support']}
        render={path='../crates/render',features=['publication-test-support']}
        client-world={package='chunk-pipeline',path='../crates/chunk-pipeline',features=['publication-test-support']}
    "#,
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn fixture_features_cannot_be_enabled_by_default_or_default_aliases() {
    for (name, feature) in [
        ("client-ui", "test-support"),
        ("chunk-pipeline", "publication-test-support"),
        ("render", "publication-test-support"),
    ] {
        for enabled in [feature, "fixtures"] {
            let temp = fixture();
            set_dependencies(
                temp.path(),
                name,
                &format!("[features]\ndefault=['{enabled}']\nfixtures=['{feature}']\n{feature}=[]"),
            );
            assert!(diagnostics(temp.path()).iter().any(|line| line
                == &format!("{name}: test-support feature `{feature}` is enabled by default")));
        }
    }
}

#[test]
fn production_dependency_kinds_cannot_enable_fixture_features_or_aliases() {
    for kind in ["dependencies", "build-dependencies"] {
        for prefix in ["", "target.'cfg(windows)'."] {
            for feature in ["test-support", "fixtures"] {
                let temp = fixture();
                let root = temp.path();
                set_dependencies(
                    root,
                    "client-ui",
                    "[features]\nfixtures=['test-support']\ntest-support=[]",
                );
                write(
                    &root.join("app/Cargo.toml"),
                    &format!(
                        "[package]\nname='bedrock-client'\nversion='0.1.0'\n[{prefix}{kind}]\nscreens={{package='client-ui',path='../crates/client-ui',features=['{feature}']}}\n"
                    ),
                );
                assert!(diagnostics(root).iter().any(|line| line == "app: production dependency `client-ui` enables test-support feature `test-support`"));
            }
        }
    }
}

#[test]
fn workspace_features_and_local_feature_additions_keep_the_dev_only_rule() {
    for (workspace_features, local_features) in [
        ("features=['test-support'],", ""),
        ("", ",features=['test-support']"),
    ] {
        let temp = fixture();
        let root = temp.path();
        let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        write(
            &root.join("Cargo.toml"),
            &format!(
                "{manifest}\n[workspace.dependencies]\nscreens={{package='client-ui',path='crates/client-ui',{workspace_features}}}\n"
            ),
        );
        write(
            &root.join("app/Cargo.toml"),
            &format!(
                "[package]\nname='bedrock-client'\nversion='0.1.0'\n[dependencies]\nscreens={{workspace=true{local_features}}}\n"
            ),
        );
        assert!(diagnostics(root).iter().any(|line| line == "app: production dependency `client-ui` enables test-support feature `test-support`"));
    }
}

#[test]
fn dependency_feature_forwarding_cannot_expose_test_support() {
    for request in ["screens/test-support", "screens?/fixtures"] {
        let temp = fixture();
        let root = temp.path();
        set_dependencies(
            root,
            "client-ui",
            "[features]\nfixtures=['test-support']\ntest-support=[]",
        );
        write(
            &root.join("app/Cargo.toml"),
            &format!(
                "[package]\nname='bedrock-client'\nversion='0.1.0'\n[dependencies]\nscreens={{package='client-ui',path='../crates/client-ui'}}\n[features]\nmore=['{request}']\n"
            ),
        );
        assert!(diagnostics(root).iter().any(|line| line == "app: production dependency `client-ui` enables test-support feature `test-support`"));
    }
}

#[test]
fn stream_facade_alias_cannot_enable_publication_fixtures_in_production() {
    for kind in ["dependencies", "build-dependencies"] {
        for prefix in ["", "target.'cfg(windows)'."] {
            let temp = fixture();
            write(
                &temp.path().join("app/Cargo.toml"),
                &format!(
                    "[package]\nname='bedrock-client'\nversion='0.1.0'\n[{prefix}{kind}]\nclient-world={{package='chunk-pipeline',path='../crates/chunk-pipeline',features=['publication-test-support']}}\n"
                ),
            );
            assert!(diagnostics(temp.path()).iter().any(|line| line
                == "app: production dependency `chunk-pipeline` enables test-support feature `publication-test-support`"));
        }
    }
}

#[test]
fn reusable_compilation_stays_out_of_ui_production_and_launcher_dependencies() {
    for kind in ["dependencies", "build-dependencies"] {
        let temp = fixture();
        set_dependencies(
            temp.path(),
            "client-ui",
            &format!("[{kind}]\ncompiler={{package='pack-compiler',path='../pack-compiler'}}"),
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line == "client-ui: forbidden dependency `pack-compiler`")
        );
    }
    for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
        let temp = fixture();
        set_dependencies(
            temp.path(),
            "launcher",
            &format!("[{kind}]\ncompiler={{package='pack-compiler',path='../pack-compiler'}}"),
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| line
                    == "launcher: forbidden dependency path `launcher -> pack-compiler`")
        );
    }
}
