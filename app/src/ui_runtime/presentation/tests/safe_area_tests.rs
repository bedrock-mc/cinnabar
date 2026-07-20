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
fn insets_reposition_the_hud_inside_the_safe_viewport_exactly() {
    // (physical, dpi, preference, expected GUI scale k, logical insets).
    // The crosshair centers exactly on the safe viewport and the hotbar
    // anchors exactly to the safe bottom edge, at fixed scales 2/3/4 and
    // Auto, including a preference clamped down by the auto rule.
    for (physical, dpi, preference, k, safe_area) in [
        (
            [1280u32, 720u32],
            1.0f32,
            None,
            3.0f32,
            insets(20.0, 10.0, 40.0, 30.0),
        ),
        ([1920, 1080], 1.0, Some(2), 2.0, insets(0.0, 0.0, 0.0, 60.0)),
        (
            [2560, 1440],
            2.0,
            Some(4),
            4.0,
            insets(30.0, 20.0, 10.0, 0.0),
        ),
        ([1280, 720], 1.0, Some(3), 3.0, insets(8.0, 8.0, 8.0, 8.0)),
        ([1366, 768], 1.0, Some(4), 3.0, insets(12.0, 0.0, 0.0, 12.0)),
    ] {
        let mut presentation = hud_presentation(preference, safe_area);
        let mut runtime = UiRuntime::new(1);
        runtime.publish_player_game_mode(PlayerGameMode::Survival);
        runtime.set_local_selected_slot(2);
        let input = presentation
            .build(&runtime, 0, physical, ui::DpiScale::new(dpi).unwrap())
            .unwrap();

        let logical = [physical[0] as f32 / dpi, physical[1] as f32 / dpi];
        let safe_center = [
            (safe_area.left() + (logical[0] - safe_area.left() - safe_area.right()) / 2.0) * dpi,
            (safe_area.top() + (logical[1] - safe_area.top() - safe_area.bottom()) / 2.0) * dpi,
        ];
        // The crosshair is the sole invert-blended quad; it spans 15k
        // physical px and centers exactly on the safe viewport, not the
        // framebuffer.
        let crosshair = input
            .batches
            .iter()
            .find(|batch| batch.blend_mode == render::UI_BLEND_INVERT)
            .expect("first person renders the invert-blended crosshair");
        let first_vertex = input.indices[crosshair.first_index as usize] as usize;
        let quad = &input.vertices[first_vertex..first_vertex + 4];
        let left = quad
            .iter()
            .map(|v| v.position[0])
            .fold(f32::INFINITY, f32::min);
        let right = quad
            .iter()
            .map(|v| v.position[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let top = quad
            .iter()
            .map(|v| v.position[1])
            .fold(f32::INFINITY, f32::min);
        let bottom = quad
            .iter()
            .map(|v| v.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(
            right - left,
            15.0 * k,
            "width at {physical:?} {safe_area:?}"
        );
        assert_eq!(
            (left + right) / 2.0,
            safe_center[0],
            "exact safe horizontal center at {physical:?} dpi {dpi} scale {k}"
        );
        assert_eq!(
            (top + bottom) / 2.0,
            safe_center[1],
            "exact safe vertical center at {physical:?} dpi {dpi} scale {k}"
        );

        // Every HUD vertex stays inside the safe rect; the sole pinned
        // exception is the 24 px selected-slot cap on the 22 px hotbar,
        // whose bottom row overhangs the safe edge by exactly 1 GUI px.
        let safe_rect = [
            safe_area.left() * dpi,
            safe_area.top() * dpi,
            (logical[0] - safe_area.right()) * dpi,
            (logical[1] - safe_area.bottom()) * dpi,
        ];
        let mut max_bottom = f32::NEG_INFINITY;
        for vertex in input.vertices.iter() {
            assert!(
                vertex.position[0] >= safe_rect[0] - 1e-3
                    && vertex.position[0] <= safe_rect[2] + 1e-3
                    && vertex.position[1] >= safe_rect[1] - 1e-3
                    && vertex.position[1] <= safe_rect[3] + k + 1e-3,
                "vertex {:?} outside safe rect {safe_rect:?} at {physical:?}",
                vertex.position
            );
            max_bottom = max_bottom.max(vertex.position[1]);
        }
        assert_eq!(
            max_bottom,
            safe_rect[3] + k,
            "the hotbar anchors to the safe bottom, the cap overhangs 1 GUI px, at {physical:?}"
        );
    }
}

#[test]
fn too_short_or_over_inset_viewports_fail_closed_to_no_hud() {
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
        runtime.publish_player_game_mode(PlayerGameMode::Survival);
        runtime.set_local_selected_slot(2);
        let input = presentation
            .build(&runtime, 0, physical, ui::DpiScale::new(dpi).unwrap())
            .unwrap();
        assert!(
            input.vertices.is_empty(),
            "no HUD renders in an unsafe viewport {physical:?} {safe_area:?}"
        );
    }
}

#[test]
fn render_input_carries_the_physical_safe_area_insets() {
    let mut presentation = hud_presentation(None, insets(10.0, 20.0, 30.0, 40.0));
    let runtime = UiRuntime::new(1);
    let input = presentation
        .build(&runtime, 0, [1280, 720], ui::DpiScale::new(1.5).unwrap())
        .unwrap();
    assert_eq!(input.viewport_size, [1280, 720]);
    assert_eq!(
        input.safe_area,
        [15, 30, 45, 60],
        "logical insets reach the render viewport as physical px"
    );
}
