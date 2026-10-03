//! Joining shows vanilla's world-loading progress screen for the dimension.

use json_ui::Draw;

use super::engine_hud_tests::{engine_presentation, engine_presentation_with};
use super::*;
use crate::ui_runtime::presentation::LoadingStage;

fn texts(presentation: &UiPresentationRuntime) -> Vec<String> {
    presentation
        .loading_draw_nodes()
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn has_sprite(presentation: &UiPresentationRuntime, wanted: &str) -> bool {
    presentation
        .loading_draw_nodes()
        .iter()
        .any(|node| matches!(&node.draw, Draw::Sprite { texture, .. } if texture == wanted))
}

#[test]
fn loading_screen_names_the_join_stage_over_the_dimensions_backdrop() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    let runtime = UiRuntime::new(1);
    presentation.set_loading_stage(Some(LoadingStage::Connecting));
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let shown = texts(&presentation);
    assert!(
        shown.iter().any(|text| text == "Locating server"),
        "{shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|text| text == "Connecting to external server")
    );
    assert!(has_sprite(&presentation, "textures/blocks/dirt"));
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    presentation.hud_frame_mut().dimension = 1;
    presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let shown = texts(&presentation);
    for wanted in ["Generating World", "Building terrain"] {
        assert!(shown.iter().any(|text| text == wanted), "{shown:?}");
    }
    assert!(has_sprite(&presentation, "textures/blocks/netherrack"));
}

/// Local-only: writes `loading_screen.png` when `CINNABAR_FORM_SNAPSHOT_DIR` is
/// set, over `CINNABAR_FORM_PACK_DIR`'s server pack when that is set too.
#[test]
fn loading_screen_snapshot() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        return;
    };
    // Vanilla art the carrier lacks (the dirt backdrop, the title) reads from
    // the local pack, as an install does.
    if let Ok(layout) = crate::install_layout::InstallLayout::discover() {
        presentation.set_vanilla_texture_root(layout.vanilla_pack_dir());
    }
    if let Some(pack) = super::super::forms::pack_harness::env_pack() {
        presentation.set_server_ui_pack(&pack);
    }
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    let build = |presentation: &mut UiPresentationRuntime| {
        presentation
            .build(
                &player_runtime,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
    };
    // The first frame places pack textures; the second draws their full-resolution copies.
    build(&mut presentation);
    presentation.finish_menu_artwork();
    let input = build(&mut presentation);
    super::super::forms::snapshot::write(&input, "loading_screen");
    if std::env::var_os("CINNABAR_FORM_PACK_DIR").is_some() {
        presentation.drop_full_res_art();
        let input = build(&mut presentation);
        super::super::forms::snapshot::write(&input, "loading_screen-server-page");
    }
}

// The overworld backdrop carries vanilla's darkening gradient and its colours.
#[test]
fn overworld_backdrop_draws_its_gradient() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = engine_presentation() else {
        return;
    };
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    presentation
        .build(
            &player_runtime,
            &UiRuntime::new(1),
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let nodes = presentation.loading_draw_nodes();
    assert!(nodes.iter().any(|node| matches!(
        &node.draw,
        Draw::Custom { renderer, data } if renderer == "gradient_renderer"
            && data.contains_key("color1") && data.contains_key("color2")
    )));
}

/// Local-only: `loading_screen_pack.png` under the pack `CINNABAR_FORM_PACK_DIR` names.
#[test]
fn loading_screen_pack_snapshot() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(pack) = super::super::forms::pack_harness::env_pack() else {
        return;
    };
    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        return;
    };
    if let Ok(layout) = crate::install_layout::InstallLayout::discover() {
        presentation.set_vanilla_texture_root(layout.vanilla_pack_dir());
    }
    presentation.set_server_ui_pack(&pack);
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    let input = presentation
        .build(
            &player_runtime,
            &UiRuntime::new(1),
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "loading_screen_pack");
}
