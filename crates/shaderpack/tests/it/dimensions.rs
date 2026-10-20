//! Dimension folders: each one replaces the root program set as a whole, as in Iris.

use crate::{open_fixture, program, reflection, texture_names};
use shaderpack::{Environment, ProgramGroup, ShaderPack, StageKind, TranslateConfig};

fn stage_paths(stages: &[(StageKind, String)]) -> Vec<&str> {
    stages.iter().map(|(_, p)| p.as_str()).collect()
}

fn config(pack: &ShaderPack, dimension: &str) -> TranslateConfig {
    TranslateConfig {
        folder: pack.dimension_folder(dimension, &Environment::default()),
        ..TranslateConfig::default()
    }
}

#[test]
fn vanilla_dimension_ids_map_to_their_folders_or_the_base_set() {
    let Some(pack) = open_fixture("dimensions") else {
        return;
    };
    let environment = Environment::default();
    assert_eq!(
        pack.dimension_folder("minecraft:overworld", &environment),
        "world0"
    );
    assert_eq!(
        pack.dimension_folder("minecraft:the_nether", &environment),
        "world-1"
    );
    // The pack has no `world1`, so the End uses the base set, which is `world0`.
    assert_eq!(
        pack.dimension_folder("minecraft:the_end", &environment),
        "world0"
    );
}

#[test]
fn the_overworld_translates_only_its_folder_programs() {
    let Some(pack) = open_fixture("dimensions") else {
        return;
    };
    let t = pack.translate(&config(&pack, "minecraft:overworld"));
    let listed: Vec<(&str, &str, Vec<&str>)> = t
        .programs
        .iter()
        .map(|p| {
            (
                p.program.name.as_str(),
                p.program.folder.as_str(),
                stage_paths(&p.program.stages),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [(
            "composite",
            "world0",
            vec!["world0/composite.vsh", "world0/composite.fsh"]
        )],
        "neither the root programs nor the nether-only final pass are loaded"
    );
    assert_eq!(
        texture_names(reflection(program(&t, "composite"))),
        ["colortex0", "colortex4"]
    );
}

#[test]
fn the_nether_translates_its_folder_and_the_root_set_stands_alone() {
    let Some(pack) = open_fixture("dimensions") else {
        return;
    };
    let t = pack.translate(&config(&pack, "minecraft:the_nether"));
    let listed: Vec<(&str, &str)> = t
        .programs
        .iter()
        .map(|p| (p.program.name.as_str(), p.program.folder.as_str()))
        .collect();
    assert_eq!(listed, [("final", "world-1")]);
    let last = program(&t, "final");
    assert_eq!(last.program.group, ProgramGroup::Final);
    assert_eq!(reflection(last).color_outputs[0].buffer, Some(0));

    let root = TranslateConfig {
        folder: String::new(),
        ..TranslateConfig::default()
    };
    let t = pack.translate(&root);
    let listed: Vec<(&str, &str)> = t
        .programs
        .iter()
        .map(|p| (p.program.name.as_str(), p.program.folder.as_str()))
        .collect();
    assert_eq!(listed, [("gbuffers_basic", ""), ("composite", "")]);
    assert!(t.programs.iter().all(|p| p.is_ok()));
    let composite = program(&t, "composite");
    assert_eq!(
        stage_paths(&composite.program.stages),
        ["composite.vsh", "composite.fsh"]
    );
    assert_eq!(texture_names(reflection(composite)), ["colortex0"]);
}
