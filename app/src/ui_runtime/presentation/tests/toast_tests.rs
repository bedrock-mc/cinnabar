//! Toast rows remain independent from the gameplay HUD node count and fail
//! closed when the safe content viewport cannot contain their top edge.

use std::sync::Arc;

use protocol::{HudEvent, PlayerGameMode, UiEvent};
use ui::{BoundedStat, DpiScale, SafeArea};

use super::{fixture_font, fixture_hud, vertical_bounds};
use crate::ui_runtime::presentation::UiPresentationRuntime;
use crate::ui_runtime::{SequencedUiEvent, UiRuntime};

fn push_toast(runtime: &mut UiRuntime, fifo_sequence: u64) {
    runtime
        .apply(SequencedUiEvent {
            session_id: 1,
            fifo_sequence,
            local_millis: 0,
            server_tick: None,
            event: UiEvent::Hud(HudEvent::Toast {
                title: Arc::from("0"),
                message: Arc::from("2"),
            }),
        })
        .unwrap();
}

fn assert_row_top(vertices: &[render::UiRenderVertex], expected: f32) {
    let (top, _) = vertical_bounds(vertices);
    assert_eq!(top, expected);
}

#[test]
fn populated_survival_hud_does_not_offset_or_reject_remote_toast_rows() {
    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.publish_player_game_mode(PlayerGameMode::Survival);
    runtime.hud.set_stats(
        BoundedStat::new(20, 20),
        BoundedStat::new(20, 20),
        BoundedStat::new(20, 20),
        None,
    );

    let baseline = presentation
        .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(
        baseline.vertices.len() / 4 >= 60,
        "fixture must exercise the populated-HUD crash threshold"
    );

    push_toast(&mut runtime, 1);
    let with_toast = presentation
        .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .expect("a routine remote toast must not make presentation fatal");
    assert_eq!(with_toast.vertices.len(), baseline.vertices.len() + 16);
    let toast = &with_toast.vertices[baseline.vertices.len()..];
    assert_row_top(&toast[..8], 12.0);
    assert_row_top(&toast[8..], 30.0);
}

#[test]
fn multiple_toasts_use_at_most_eight_consecutive_text_rows() {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let mut runtime = UiRuntime::new(1);
    for fifo_sequence in 1..=5 {
        push_toast(&mut runtime, fifo_sequence);
    }

    let input = presentation
        .build(&runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    assert_eq!(input.vertices.len(), 8 * 8);
    for (row, vertices) in input.vertices.chunks_exact(8).enumerate() {
        assert_row_top(vertices, 12.0 + row as f32 * 18.0);
    }
}

#[test]
fn toast_rows_below_safe_content_height_are_omitted_at_each_dpi() {
    for (physical_size, dpi) in [([640, 120], 1.0), ([1280, 240], 2.0)] {
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_safe_area(SafeArea::new(0.0, 20.0, 0.0, 20.0).unwrap());
        let mut runtime = UiRuntime::new(1);
        for fifo_sequence in 1..=4 {
            push_toast(&mut runtime, fifo_sequence);
        }

        let input = presentation
            .build(&runtime, 0, physical_size, DpiScale::new(dpi).unwrap())
            .expect("out-of-viewport toast rows must be skipped, not rejected");
        assert_eq!(input.vertices.len(), 4 * 8);
        assert_row_top(&input.vertices[..8], (20.0 + 12.0) * dpi);
        assert_row_top(&input.vertices[3 * 8..], (20.0 + 66.0) * dpi);
    }
}
