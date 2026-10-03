use ui::{DpiScale, SafeArea};

use super::fixture_font;
use crate::{
    menu::{MenuAction, MenuRuntime, MenuScreen},
    ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
};

const OBSERVED_KICK: &str = "server disconnected: Cinnabar launcher return check (the server ended the current play session)";

#[test]
fn home_catalog_recovery_stays_inside_the_featured_empty_card() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let runtime = UiRuntime::new(1);
    for (width, height, gui_scale) in [
        (1280, 720, 2),
        (1280, 720, 3),
        (900, 720, 2),
        (420, 720, 2),
        (420, 720, 3),
        (900, 360, 1),
        (420, 360, 1),
    ] {
        let menu = MenuRuntime::new(true, gui_scale, "Player".to_owned());
        let mut view = menu.view();
        view.catalog_loading = false;
        view.catalog_message = Some("Social: Refresh to try again.".to_owned());
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_gui_scale_preference(Some(gui_scale));
        presentation.set_menu_view(Some(view));
        let input = presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [width, height],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        // Locate actual empty-card quads rather than repeating production's
        // origin formula. Headers are checked separately, including shadows.
        let cards = input
            .vertices
            .chunks_exact(4)
            .filter(|quad| quad[0].color == [33, 42, 56, 252])
            .map(|quad| {
                quad.iter().fold(
                    (
                        f32::INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::NEG_INFINITY,
                    ),
                    |(left, top, right, bottom), vertex| {
                        (
                            left.min(vertex.position[0]),
                            top.min(vertex.position[1]),
                            right.max(vertex.position[0]),
                            bottom.max(vertex.position[1]),
                        )
                    },
                )
            })
            .collect::<Vec<_>>();
        if height == 720 {
            assert_eq!(cards.len(), if width >= 900 { 2 } else { 1 });
        }
        for (left, top, right, bottom) in cards {
            assert!(
                left >= 0.0
                    && right <= width as f32
                    && top >= 0.0
                    && bottom <= height as f32 - 16.0,
                "card escaped {width}x{height}/GUI{gui_scale}: {left},{top}..{right},{bottom}"
            );
            let body: Vec<_> = input
                .vertices
                .chunks_exact(4)
                .filter(|quad| {
                    let min_x = quad
                        .iter()
                        .map(|vertex| vertex.position[0])
                        .fold(f32::INFINITY, f32::min);
                    let min_y = quad
                        .iter()
                        .map(|vertex| vertex.position[1])
                        .fold(f32::INFINITY, f32::min);
                    quad[0].color == [166, 178, 193, 255]
                        && min_x >= left + 16.0
                        && min_x < right
                        && min_y >= top + 16.0
                        && min_y < bottom
                })
                .flat_map(|quad| quad.iter())
                .collect();
            assert!(
                !body.is_empty(),
                "recovery text must remain visible at GUI {gui_scale}"
            );
            assert!(
                body.iter()
                    .all(|vertex| vertex.position[0] <= right && vertex.position[1] <= bottom),
                "recovery text escaped its card at GUI {gui_scale}"
            );
            let title_bottom = input
                .vertices
                .chunks_exact(4)
                .filter(|quad| {
                    let min_x = quad
                        .iter()
                        .map(|vertex| vertex.position[0])
                        .fold(f32::INFINITY, f32::min);
                    let min_y = quad
                        .iter()
                        .map(|vertex| vertex.position[1])
                        .fold(f32::INFINITY, f32::min);
                    quad[0].color == [239, 243, 247, 255]
                        && min_x >= left + 16.0
                        && min_x < right
                        && min_y >= top + 16.0
                        && min_y < bottom
                })
                .flat_map(|quad| quad.iter())
                .map(|vertex| vertex.position[1])
                .fold(f32::NEG_INFINITY, f32::max);
            let body_top = body
                .iter()
                .map(|vertex| vertex.position[1])
                .fold(f32::INFINITY, f32::min);
            if height == 720 {
                assert!(
                    title_bottom.is_finite(),
                    "the full-height card must retain its title"
                );
            }
            assert!(
                body_top >= title_bottom + 6.0,
                "title and recovery need a readable gap at {width}x{height}/GUI{gui_scale}: card={left},{top}..{right},{bottom}, title_bottom={title_bottom}, body_top={body_top}"
            );
            let text_and_shadow = input
                .vertices
                .chunks_exact(4)
                .filter(|quad| {
                    let min_x = quad
                        .iter()
                        .map(|vertex| vertex.position[0])
                        .fold(f32::INFINITY, f32::min);
                    let min_y = quad
                        .iter()
                        .map(|vertex| vertex.position[1])
                        .fold(f32::INFINITY, f32::min);
                    min_x >= left + 16.0 && min_x < right && min_y >= top + 16.0 && min_y < bottom
                })
                .flat_map(|quad| quad.iter())
                .collect::<Vec<_>>();
            assert!(
                text_and_shadow
                    .iter()
                    .all(|vertex| vertex.position[0] <= right && vertex.position[1] <= bottom),
                "whole text/shadow quad escaped card at {width}x{height}/GUI{gui_scale}"
            );
            // A glyph/shadow quad whose origin precedes the card must finish
            // before it. This catches the old two-row heading overlap directly.
            let header_bottom = input
                .vertices
                .chunks_exact(4)
                .filter(|quad| {
                    let min_y = quad
                        .iter()
                        .map(|vertex| vertex.position[1])
                        .fold(f32::INFINITY, f32::min);
                    let min_x = quad
                        .iter()
                        .map(|vertex| vertex.position[0])
                        .fold(f32::INFINITY, f32::min);
                    min_y >= if width >= 900 { 98.0 } else { 340.0 }
                        && min_y < top
                        && min_x >= left
                        && min_x < right
                        && !matches!(
                            quad[0].color,
                            [33, 42, 56, 252] | [26, 33, 45, 250] | [55, 67, 85, 255]
                        )
                })
                .flat_map(|quad| quad.iter())
                .map(|vertex| vertex.position[1])
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(
                header_bottom + 6.0 <= top,
                "header overlaps recovery card at {width}x{height}/GUI{gui_scale}: header_bottom={header_bottom}, card_top={top}"
            );
        }
    }
}

#[test]
fn measured_home_heading_never_moves_successful_featured_hits_below_the_viewport() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let runtime = UiRuntime::new(1);
    for (width, height, gui) in [(1280, 720, 3), (900, 360, 1)] {
        let mut view = MenuRuntime::new(true, gui, "Player".to_owned()).view();
        view.catalog_loading = false;
        view.catalog_message = Some("Social: Refresh to try again.".to_owned());
        view.featured = (0..3)
            .map(|index| crate::menu::MenuServerCard {
                name: format!("Server {index}"),
                address: "example.invalid".to_owned(),
                caption: "Available".to_owned(),
                image_path: String::new(),
                icon: None,
            })
            .collect();
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_gui_scale_preference(Some(gui));
        presentation.set_menu_view(Some(view));
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [width, height],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let hits = presentation
            .menu_hit_targets
            .iter()
            .filter(|(action, _)| matches!(action, MenuAction::PlayFeatured(_)))
            .collect::<Vec<_>>();
        assert!(
            !hits.is_empty(),
            "successful catalog content remains actionable"
        );
        assert!(
            hits.iter()
                .all(|(_, bounds)| bounds.max().y() <= height as f32 - 16.0),
            "an omitted row must not leave an offscreen hit target"
        );
        if height == 720 {
            assert_eq!(hits.len(), 3);
        } else {
            assert!(hits.len() < 3);
        }
    }
}

fn presented_message(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    physical_size: [u32; 2],
    safe_area: SafeArea,
    reason: &str,
) -> (render::UiRenderInput, usize) {
    let runtime = UiRuntime::new(1);
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    menu.activate(MenuAction::Navigate(MenuScreen::Play));
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.set_safe_area(safe_area);
    let mut baseline_view = menu.view();
    baseline_view.message = None;
    presentation.set_menu_view(Some(baseline_view));
    let baseline = presentation
        .build(
            player_runtime,
            &runtime,
            0,
            physical_size,
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();

    let mut view = menu.view();
    view.message = Some(reason.to_owned());
    presentation.set_menu_view(Some(view));
    let active = presentation
        .build(
            player_runtime,
            &runtime,
            0,
            physical_size,
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    (active, baseline.vertices.len())
}

#[test]
fn observed_launcher_disconnect_message_stays_inside_the_window() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let (active, baseline_vertices) =
        presented_message(&player_runtime, [1280, 720], SafeArea::ZERO, OBSERVED_KICK);
    let status = &active.vertices[baseline_vertices..];

    assert!(
        status.len() > 8,
        "status must retain its text after two panel quads"
    );
    assert!(
        status.iter().all(|vertex| vertex.position[1] <= 720.0),
        "launcher status escaped the window: {:?}",
        status
            .iter()
            .map(|vertex| vertex.position[1])
            .fold(f32::NEG_INFINITY, f32::max)
    );
    let (panel_top, panel_bottom) = status[..4].iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(top, bottom), vertex| (top.min(vertex.position[1]), bottom.max(vertex.position[1])),
    );
    assert!(
        panel_bottom - panel_top > 48.0,
        "multi-row status panel did not expand upward from its former fixed height"
    );
}

#[test]
fn long_launcher_status_keeps_only_complete_rows_inside_a_narrow_safe_viewport() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let safe_area = SafeArea::new(18.0, 24.0, 22.0, 36.0).unwrap();
    let reason = "bounded launcher status row ".repeat(40);
    let (active, baseline_vertices) =
        presented_message(&player_runtime, [420, 360], safe_area, &reason);
    let status = &active.vertices[baseline_vertices..];
    let safe_bottom = 360.0 - safe_area.bottom();

    assert!(
        status.len() > 8,
        "the constrained viewport still fits status text"
    );
    assert!(
        status
            .iter()
            .all(|vertex| vertex.position[1] <= safe_bottom),
        "status text or panel escaped the safe viewport"
    );
    assert!(
        status.iter().all(|vertex| {
            vertex.position[0] >= safe_area.left()
                && vertex.position[0] <= 420.0 - safe_area.right()
        }),
        "status text or panel escaped the narrow safe width"
    );

    let text = &status[8..];
    let lowest_row_top = text
        .chunks_exact(4)
        .map(|quad| {
            quad.iter()
                .map(|vertex| vertex.position[1])
                .fold(f32::INFINITY, f32::min)
        })
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        text.iter()
            .filter(|vertex| vertex.position[1] >= lowest_row_top)
            .all(|vertex| vertex.position[1] <= safe_bottom),
        "the final retained status row must be wholly visible"
    );
}
