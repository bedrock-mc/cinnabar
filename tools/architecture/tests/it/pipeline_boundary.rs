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
        &root
            .join(if name == "app" {
                "app".to_owned()
            } else {
                format!("crates/{name}")
            })
            .join("Cargo.toml"),
        &format!("[package]\nname='{name}'\nversion='0.1.0'\n{dependencies}\n"),
    );
}

/// Return all diagnostics from the fixture's copy of the real policy.
fn diagnostics(root: &Path) -> Vec<String> {
    check_repository(root, &root.join("policy.toml")).expect("check fixture")
}

#[test]
fn pipeline_and_compiler_accept_only_their_lower_production_dependencies() {
    let temp = fixture();
    let root = temp.path();
    set_dependencies(
        root,
        "client-world",
        "[dependencies]\nprotocol={path='../protocol'}\nworld={path='../world'}\nassets={path='../assets'}",
    );
    set_dependencies(
        root,
        "protocol",
        "[dependencies]\nrender-api={path='../render-api'}",
    );
    set_dependencies(
        root,
        "chunk-pipeline",
        "[dependencies]\nclient-world={path='../client-world'}\nmeshing={path='../meshing'}\nassets={path='../assets'}\nworld={path='../world'}\nrender-api={path='../render-api'}\n[dev-dependencies]\nprotocol={path='../protocol'}",
    );
    set_dependencies(
        root,
        "pack-compiler",
        "[dependencies]\nassets={path='../assets'}",
    );
    set_dependencies(
        root,
        "asset-compiler",
        "[dependencies]\nassets={path='../assets'}\npack-compiler={path='../pack-compiler'}",
    );
    set_dependencies(
        root,
        "app",
        "[dependencies]\nclient-world={package='chunk-pipeline',path='../crates/chunk-pipeline'}\npack-compiler={path='../crates/pack-compiler'}",
    );
    assert_eq!(diagnostics(root), Vec::<String>::new());
}

#[test]
fn client_world_rejects_renamed_pipeline_and_compiler_edges_including_tests() {
    for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
        for prefix in ["", "target.'cfg(windows)'."] {
            for name in [
                "chunk-pipeline",
                "pack-compiler",
                "asset-compiler",
                "meshing",
            ] {
                let temp = fixture();
                set_dependencies(
                    temp.path(),
                    "client-world",
                    &format!("[{prefix}{kind}]\nupper={{package='{name}',path='../{name}'}}"),
                );
                assert!(
                    diagnostics(temp.path()).iter().any(|line| {
                        line == &format!(
                            "client-world: forbidden dependency path `client-world -> {name}`"
                        )
                    }),
                    "missed {prefix}{kind} edge to {name}"
                );
            }
        }
    }
}

#[test]
fn client_world_cannot_import_the_publication_contract_directly() {
    for kind in ["dependencies", "build-dependencies"] {
        let temp = fixture();
        set_dependencies(
            temp.path(),
            "client-world",
            &format!("[{kind}]\ncontracts={{package='render-api',path='../render-api'}}"),
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| { line == "client-world: forbidden dependency `render-api`" })
        );
    }
}

#[test]
fn production_pipeline_cannot_parse_protocol_via_a_renamed_dependency() {
    for kind in ["dependencies", "build-dependencies"] {
        let temp = fixture();
        set_dependencies(
            temp.path(),
            "chunk-pipeline",
            &format!("[{kind}]\nwire={{package='protocol',path='../protocol'}}"),
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| { line == "chunk-pipeline: forbidden dependency `protocol`" })
        );
    }
}

#[test]
fn new_domain_crates_reject_hidden_engine_dependencies() {
    for name in ["chunk-pipeline", "pack-compiler", "client-world"] {
        for package in ["bevy", "bevy_ecs", "wgpu", "wgpu-core"] {
            let temp = fixture();
            set_dependencies(
                temp.path(),
                name,
                "[dependencies]\nassets={path='../assets'}",
            );
            set_dependencies(
                temp.path(),
                "assets",
                &format!(
                    "[target.'cfg(windows)'.build-dependencies]\nengine={{package='{package}',version='1'}}"
                ),
            );
            assert!(
                diagnostics(temp.path()).iter().any(|line| {
                    line == &format!(
                        "{name}: forbidden dependency path `{name} -> assets -> {package}`"
                    )
                }),
                "missed {name} -> assets -> {package}"
            );
        }
    }
}

#[test]
fn app_cannot_regain_the_offline_compiler_through_runtime_or_test_edges() {
    for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
        let temp = fixture();
        set_dependencies(
            temp.path(),
            "app",
            &format!(
                "[{kind}]\noffline={{package='asset-compiler',path='../crates/asset-compiler'}}"
            ),
        );
        assert!(
            diagnostics(temp.path())
                .iter()
                .any(|line| { line == "app: forbidden dependency path `app -> asset-compiler`" })
        );
    }
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "app",
        "[dependencies]\npack-compiler={path='../crates/pack-compiler'}",
    );
    set_dependencies(
        temp.path(),
        "pack-compiler",
        "[dependencies]\noffline={package='asset-compiler',path='../asset-compiler'}",
    );
    assert!(diagnostics(temp.path()).iter().any(|line| {
        line == "app: forbidden dependency path `app -> pack-compiler -> asset-compiler`"
    }));
}

#[test]
fn reusable_compilation_cannot_depend_on_cli_or_world_runtime() {
    for name in [
        "asset-compiler",
        "client-world",
        "chunk-pipeline",
        "protocol",
        "clap",
    ] {
        let temp = fixture();
        let dependency = if name == "clap" {
            "cli={package='clap',version='1'}".to_owned()
        } else {
            format!("runtime={{package='{name}',path='../{name}'}}")
        };
        set_dependencies(
            temp.path(),
            "pack-compiler",
            &format!("[dev-dependencies]\n{dependency}"),
        );
        assert!(
            diagnostics(temp.path()).iter().any(|line| {
                line == &format!(
                    "pack-compiler: forbidden dependency path `pack-compiler -> {name}`"
                )
            }),
            "missed reusable compiler -> {name}"
        );
    }
}

#[test]
fn renderer_cannot_regain_chunk_state_through_a_test_dependency() {
    let temp = fixture();
    set_dependencies(
        temp.path(),
        "render",
        "[dev-dependencies]\nstate={package='chunk-pipeline',path='../chunk-pipeline'}",
    );
    assert!(
        diagnostics(temp.path())
            .iter()
            .any(|line| { line == "render: forbidden dependency path `render -> chunk-pipeline`" })
    );
}

#[test]
fn authoritative_state_cannot_own_meshes_or_publication_permits() {
    for source in [
        "use render_api::{PublicationPermit as Permit}; struct State { permit: Option<Permit> }",
        "pub use meshing::{ChunkMesh as Mesh}; struct State { mesh: Mesh }",
        "use crate::chunk_pipeline::WorldStream;",
        "pub use super::super::meshing as mesh;",
    ] {
        let temp = fixture();
        write(
            &temp.path().join("crates/client-world/src/state.rs"),
            source,
        );
        assert!(
            diagnostics(temp.path()).iter().any(|line| {
                line.contains("crosses forbidden") || line.contains("owns forbidden authority")
            }),
            "missed {source}"
        );
    }
}

#[test]
fn boundary_syntax_checks_ignore_comments_and_test_only_paths() {
    let temp = fixture();
    write(
        &temp.path().join("crates/client-world/src/state.rs"),
        "// use chunk_pipeline::WorldStream;\n#[cfg(test)] mod tests { use render_api::PublicationPermit; }\nconst NOTE: &str = \"meshing::ChunkMesh\";",
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}

#[test]
fn coordinator_cannot_own_a_second_mutable_world_store() {
    for source in [
        "use world::ChunkStore as Store; struct Pipeline { copy: Option<Store> }",
        "type Store = client_world::ActorStore; struct Pipeline { copy: Box<Store> }",
    ] {
        let temp = fixture();
        write(
            &temp.path().join("crates/chunk-pipeline/src/state.rs"),
            source,
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
fn coordinator_can_borrow_terrain_and_compose_the_authority_owner() {
    let temp = fixture();
    write(
        &temp.path().join("crates/chunk-pipeline/src/state.rs"),
        "struct Pipeline { authority: client_world::WorldAuthority } struct View<'a> { terrain: &'a world::ChunkStore } #[cfg(test)] struct Fixture { store: world::ChunkStore }",
    );
    assert_eq!(diagnostics(temp.path()), Vec::<String>::new());
}
