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
    let mut player_runtime = player_state::PlayerState::new(1);

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
        let runtime = UiRuntime::new(1);
        player_runtime
            .facts
            .publish_player_game_mode(PlayerGameMode::Survival);
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
    let player_runtime = player_state::PlayerState::new(1);

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

#[test]
fn tiny_launcher_text_keeps_rendering_after_resize() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let player = player_state::PlayerState::new(0);
    let runtime = UiRuntime::new(0);
    let mut view = crate::menu::MenuView::new(true, "Fixture".into());
    view.screen = crate::menu::MenuScreen::Settings;
    presentation.set_menu_view(Some(view));
    for physical in [[1280, 720], [254, 124], [1, 1], [1280, 720]] {
        let result = presentation.build(
            &player,
            &runtime,
            0,
            physical,
            ui::DpiScale::new(1.0).unwrap(),
        );
        if physical == [1, 1] {
            assert!(
                result.is_err(),
                "unfit screen geometry rejects only this frame"
            );
            continue;
        }
        let input = result.unwrap();
        assert_eq!(input.viewport_size, physical);
        if physical == [1280, 720] {
            assert!(!input.vertices.is_empty());
        }
        super::super::forms::snapshot::write(
            &input,
            &format!("tiny-text-{}x{}", physical[0], physical[1]),
        );
    }
}

#[test]
fn six_pixel_glyph_in_one_pixel_label_still_has_a_layout() {
    let font = fixture_font();
    let metrics =
        super::super::TextMetrics::for_viewport([254, 124], ui::DpiScale::new(1.0).unwrap(), None);
    let mut cache = ui::TextLayoutCache::new(8, 64 * 1024);
    let layout = cache.layout(metrics.request("W", 64, &font)).unwrap();
    assert_eq!(layout.size_64()[0], 384);
    assert_eq!(layout.glyphs().len(), 1);
    assert_eq!(cache.visual_overflow_count(), 1);
    cache.layout(metrics.request("W", 64, &font)).unwrap();
    assert_eq!(
        cache.visual_overflow_count(),
        1,
        "cached frames do not recount text"
    );
    let mut strict = metrics.request("W", 64, &font);
    strict.wrap.allow_visual_overflow = false;
    assert!(
        matches!(
            cache.layout(strict),
            Err(ui::TextError::VisualWidthExceeded {
                actual_64: 384,
                limit_64: 64
            })
        ),
        "strict requests do not reuse the tolerant layout"
    );
    let bounds = super::super::rect(0.0, 0.0, 1.0, 10.0).unwrap();
    let viewport = super::super::rect(0.0, 0.0, 254.0, 124.0).unwrap();
    let parent = ui::UiNodeId::new(1);
    let mut tree = ui::UiTree::new(vec![
        ui::UiNode::new(parent, None, bounds).with_clip_children(true),
        ui::UiNode::new(ui::UiNodeId::new(2), Some(parent), bounds).with_visual(
            ui::UiVisual::Text {
                layout,
                color: [255; 4],
                shadow: ui::TextShadow::None,
            },
        ),
    ])
    .unwrap();
    tree.layout(viewport, ui::UiScale::default(), ui::SafeArea::ZERO)
        .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert!(!draw.vertices.is_empty(), "the wide glyph reaches drawing");
    assert!(draw.batches.iter().all(|batch| batch.clip == bounds));
    let presentation = UiPresentationRuntime::new(font.clone()).unwrap();
    let input = crate::ui_runtime::render_adapter::adapt_ui_draw_list(
        &draw,
        presentation.textures.clone(),
        crate::ui_runtime::render_adapter::UiRenderViewport {
            physical_size: [254, 124],
            dpi_scale: ui::DpiScale::new(1.0).unwrap(),
            safe_area: ui::SafeArea::ZERO,
        },
    )
    .unwrap();
    let pixels = super::super::forms::snapshot::rasterize(&input);
    let background = *pixels.get_pixel(253, 123);
    assert!(
        pixels.pixels().any(|pixel| *pixel != background),
        "clipped glyph ink renders"
    );
    assert!(
        pixels
            .enumerate_pixels()
            .all(|(x, y, pixel)| *pixel == background || (x == 0 && y < 10)),
        "ink stays inside the one-pixel control"
    );
    super::super::forms::snapshot::write(&input, "overflowing-glyph-clipped");
}

#[test]
fn installed_launcher_text_snapshot() {
    let Some(mut presentation) = super::super::forms::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping installed_launcher_text_snapshot: missing local UI carrier (make assets)"
        );
        return;
    };
    let runtime = super::super::forms::pack_harness::menu_runtime();
    let player = player_state::PlayerState::new(0);
    let mut view = crate::menu::MenuView::new(true, "Fixture".into());
    view.screen = crate::menu::MenuScreen::Settings;
    presentation.set_menu_view(Some(view));
    let input = presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "installed-launcher-text-normal");
}
