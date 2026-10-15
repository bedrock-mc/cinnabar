//! World entry uses OreUI; dimension changes retain the session's JSON-UI backdrop.

use json_ui::Draw;

use super::engine_hud_tests::{engine_presentation, engine_presentation_with};
use super::*;
use ui::LoadingStage;

#[test]
fn world_entry_loading_leaves_the_menu_panorama_visible() {
    let player = player_state::PlayerState::new(1);
    let runtime = UiRuntime::new(1);
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    for stage in [LoadingStage::Connecting, LoadingStage::BuildingTerrain] {
        presentation.set_loading_stage(Some(stage));
        presentation
            .build(
                &player,
                &runtime,
                1_000,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(
            !presentation.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
                node.bounds().width() >= 1280.0
                    && node.bounds().height() >= 720.0
                    && matches!(node.visual(), ui::UiVisual::Solid { color, .. } if color[3] == 255)
            }),
            "{stage:?} must not cover the panorama with an opaque fill"
        );
    }
}

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

fn painted_sprite(presentation: &UiPresentationRuntime, wanted: &str) -> bool {
    let Some((page, region)) = presentation.loading_texture_sprite(wanted) else {
        return false;
    };
    presentation
        .last_frame
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .any(|node| {
            matches!(node.visual(), ui::UiVisual::Sprite { texture_page, uv, .. }
            if *texture_page == page && uv[0] >= region[0] && uv[1] >= region[1]
                && uv[2] <= region[2] && uv[3] <= region[3])
        })
}

fn painted_text(presentation: &UiPresentationRuntime, wanted: &str) -> bool {
    let normalized = |text: &str| {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_uppercase()
    };
    presentation
        .last_frame
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .any(|node| {
            if let ui::UiVisual::Text { layout, .. } = node.visual() {
                let text: String = layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect();
                normalized(&text) == normalized(wanted)
            } else {
                false
            }
        })
}

#[test]
fn loading_screen_names_the_join_stage_over_the_dimensions_backdrop() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping loading_screen_names_the_join_stage_over_the_dimensions_backdrop: fixture unavailable; requires installed local carriers (make assets)"
        );
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
    for wanted in ["Locating server", "Connecting to external server"] {
        assert!(painted_text(&presentation, wanted));
    }
    assert!(presentation.loading_draw_nodes().is_empty());
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
    for wanted in ["Generating World", "Building terrain"] {
        assert!(painted_text(&presentation, wanted));
    }
    assert!(presentation.loading_draw_nodes().is_empty());
}

#[test]
fn dimension_loading_names_its_destination_without_the_join_animation() {
    let player_runtime = player_state::PlayerState::new(1);
    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping dimension_loading_names_its_destination_without_the_join_animation: missing installed UI carrier; make assets"
        );
        return;
    };
    let runtime = UiRuntime::new(1);
    let frame = |presentation: &mut UiPresentationRuntime| {
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    };
    presentation.set_loading_stage(Some(LoadingStage::BuildingTerrain));
    frame(&mut presentation);
    assert!(presentation.loading_draw_nodes().is_empty());
    presentation.set_loading_stage(Some(LoadingStage::ChangingDimension));
    for (dimension, (backdrop, destination)) in [
        ("textures/blocks/dirt", "Entering the Overworld"),
        ("textures/blocks/netherrack", "Entering the Nether"),
        ("textures/blocks/end_stone", "Entering the End"),
    ]
    .into_iter()
    .enumerate()
    {
        presentation.hud_frame_mut().dimension = dimension.try_into().unwrap();
        frame(&mut presentation);
        let shown = texts(&presentation);
        for wanted in ["Generating World", "Building terrain"] {
            assert!(shown.iter().any(|text| text == wanted), "{shown:?}");
        }
        assert!(
            painted_text(&presentation, destination),
            "the painted status names its destination: {destination}"
        );
        assert!(has_sprite(&presentation, backdrop));
        assert!(!painted_sprite(&presentation, "textures/ui/loading_bar"));
        assert!(!painted_sprite(&presentation, "textures/ui/loading_spin"));
    }
    for dimension in [37, -1] {
        presentation.hud_frame_mut().dimension = dimension;
        frame(&mut presentation);
        assert!(painted_text(&presentation, "Changing dimension"));
    }
}

/// Local-only: writes `loading_screen.png` when `CINNABAR_FORM_SNAPSHOT_DIR` is
/// set, over `CINNABAR_FORM_PACK_DIR`'s server pack when that is set too.
#[test]
fn loading_screen_snapshot() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping loading_screen_snapshot: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_FORM_PACK_DIR"
        );
        return;
    };
    // Vanilla art the carrier lacks (the dirt backdrop, the title) reads from
    // the local pack, as an install does.
    if let Ok(layout) = launcher::install_layout::InstallLayout::discover() {
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

#[test]
fn terrain_loading_uses_only_native_nodes() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(mut presentation) = engine_presentation() else {
        eprintln!(
            "skipping overworld_backdrop_draws_its_gradient: fixture unavailable; requires installed local carriers (make assets)"
        );
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
    assert!(presentation.loading_draw_nodes().is_empty());
    assert!(painted_text(&presentation, "Building terrain"));
}

/// Local-only: `loading_screen_pack.png` under the pack `CINNABAR_FORM_PACK_DIR` names.
#[test]
fn loading_screen_pack_snapshot() {
    let player_runtime = player_state::PlayerState::new(1);

    let Some(pack) = super::super::forms::pack_harness::env_pack() else {
        eprintln!(
            "skipping loading_screen_pack_snapshot: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping loading_screen_pack_snapshot: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    if let Ok(layout) = launcher::install_layout::InstallLayout::discover() {
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
