//! Dimension folders and per-program fallback to the `shaders/` root.

use crate::{open_fixture, program, reflection, texture_names};
use shaderpack::{ProgramGroup, StageKind, TranslateConfig};

fn stage_paths(stages: &[(StageKind, String)]) -> Vec<&str> {
    stages.iter().map(|(_, p)| p.as_str()).collect()
}

#[test]
fn vanilla_dimension_ids_map_to_their_folders() {
    let Some(pack) = open_fixture("dimensions") else {
        return;
    };
    assert_eq!(pack.dimension_folder("minecraft:overworld"), "world0");
    assert_eq!(pack.dimension_folder("minecraft:the_nether"), "world-1");
    assert_eq!(pack.dimension_folder("minecraft:the_end"), "world1");
}

#[test]
fn overworld_programs_prefer_the_folder_and_fall_back_to_the_root() {
    let Some(pack) = open_fixture("dimensions") else {
        return;
    };
    let config = TranslateConfig {
        folder: pack.dimension_folder("minecraft:overworld"),
        ..TranslateConfig::default()
    };
    let programs = pack.programs(&config);
    let listed: Vec<(&str, &str, Vec<&str>)> = programs
        .iter()
        .map(|p| (p.name.as_str(), p.folder.as_str(), stage_paths(&p.stages)))
        .collect();
    assert_eq!(
        listed,
        [
            (
                "gbuffers_basic",
                "",
                vec!["gbuffers_basic.vsh", "gbuffers_basic.fsh"]
            ),
            (
                "composite",
                "world0",
                vec!["world0/composite.vsh", "world0/composite.fsh"]
            ),
        ],
        "the nether-only final pass is not loaded"
    );
    assert_eq!(programs[0].group, ProgramGroup::Gbuffers);
    assert_eq!(programs[1].group, ProgramGroup::Composite);

    let t = pack.translate(&config);
    assert!(t.programs.iter().all(|p| p.is_ok()));
    assert_eq!(
        texture_names(reflection(program(&t, "composite"))),
        ["colortex0", "colortex4"]
    );
}

#[test]
fn nether_uses_its_folder_and_the_root_for_the_rest() {
    let Some(pack) = open_fixture("dimensions") else {
        return;
    };
    let config = TranslateConfig {
        folder: pack.dimension_folder("minecraft:the_nether"),
        ..TranslateConfig::default()
    };
    let t = pack.translate(&config);
    let listed: Vec<(&str, &str)> = t
        .programs
        .iter()
        .map(|p| (p.program.name.as_str(), p.program.folder.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            ("gbuffers_basic", ""),
            ("composite", ""),
            ("final", "world-1")
        ]
    );
    let composite = program(&t, "composite");
    assert_eq!(
        stage_paths(&composite.program.stages),
        ["composite.vsh", "composite.fsh"]
    );
    assert_eq!(texture_names(reflection(composite)), ["colortex0"]);
    let last = program(&t, "final");
    assert_eq!(last.program.group, ProgramGroup::Final);
    assert_eq!(
        stage_paths(&last.program.stages),
        ["world-1/final.vsh", "world-1/final.fsh"]
    );
    assert_eq!(reflection(last).color_outputs[0].buffer, Some(0));
}
