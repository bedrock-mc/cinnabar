//! Safe-area inset witnesses: platform insets flow into the HUD geometry,
//! the retained layout, and the render viewport, and viewports whose safe
//! region cannot hold the fixed HUD fail closed to no HUD at all.

use protocol::PlayerGameMode;
use ui::SafeArea;

use super::{fixture_font, fixture_hud};
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::{HudFrame, UiPresentationRuntime};

fn insets(left: f32, top: f32, right: f32, bottom: f32) -> SafeArea {
    SafeArea::new(left, top, right, bottom).unwrap()
}

fn hud_presentation(preference: Option<u8>, safe_area: SafeArea) -> UiPresentationRuntime {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_gui_scale_preference(preference);
    presentation.set_safe_area(safe_area);
    *presentation.hud_frame_mut() = HudFrame {
        first_person: true,
        ..HudFrame::default()
    };
    presentation
}

#[test]
fn too_short_or_over_inset_viewports_fail_closed_to_no_hud() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    // (physical, dpi, preference, insets): each safe viewport is too narrow
    // or too short for the fixed hotbar and bottom stack.
    for (physical, dpi, preference, safe_area) in [
        // 50 GUI px tall at the auto scale of 1: shorter than the 59 px
        // bottom stack.
        ([1280u32, 50u32], 1.0f32, None, SafeArea::ZERO),
        // Insets consume the height: (720 - 600) / 3 = 40 GUI px.
        ([1280, 720], 1.0, None, insets(0.0, 400.0, 0.0, 200.0)),
        // Insets consume the width: 600 - 430 = 170 < 182 GUI px at k = 1.
        ([600, 720], 1.0, None, insets(250.0, 0.0, 180.0, 0.0)),
    ] {
        let mut presentation = hud_presentation(preference, safe_area);
        let mut runtime = UiRuntime::new(1);
        runtime.publish_player_game_mode(&mut player_runtime, PlayerGameMode::Survival);
        player_runtime.inventory.set_local_selected_slot(2);
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                physical,
                ui::DpiScale::new(dpi).unwrap(),
            )
            .unwrap();
        assert!(
            input.vertices.is_empty(),
            "no HUD renders in an unsafe viewport {physical:?} {safe_area:?}"
        );
    }
}

#[test]
fn render_input_carries_the_physical_safe_area_insets() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut presentation = hud_presentation(None, insets(10.0, 20.0, 30.0, 40.0));
    let runtime = UiRuntime::new(1);
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.5).unwrap(),
        )
        .unwrap();
    assert_eq!(input.viewport_size, [1280, 720]);
    assert_eq!(
        input.safe_area,
        [15, 30, 45, 60],
        "logical insets reach the render viewport as physical px"
    );
}
