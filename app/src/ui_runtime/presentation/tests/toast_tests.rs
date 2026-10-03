//! Server toasts through vanilla `toast_screen.json`: the popup slides down
//! from above the top edge, holds, slides back, and the next one follows.

use std::sync::Arc;

use json_ui::{Draw, DrawNode};
use protocol::{HudEvent, UiEvent};
use ui::DpiScale;

use crate::ui_runtime::{SequencedUiEvent, UiRuntime, presentation::UiPresentationRuntime};

fn push_toast(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    runtime: &mut UiRuntime,
    fifo_sequence: u64,
    title: &str,
    message: &str,
) {
    runtime
        .apply(
            player_runtime,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Hud(HudEvent::Toast {
                    title: Arc::from(title),
                    message: Arc::from(message),
                }),
            },
        )
        .unwrap();
}

fn text<'a>(nodes: &'a [DrawNode], wanted: &str) -> Option<&'a DrawNode> {
    nodes
        .iter()
        .find(|node| matches!(&node.draw, Draw::Text { text, .. } if text == wanted))
}

fn build(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now: u64,
) {
    presentation
        .build(
            player_runtime,
            runtime,
            now,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .expect("a remote toast never makes presentation fatal");
}

#[test]
fn server_toast_slides_down_from_the_top_holds_then_yields_to_the_next() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = super::engine_hud_tests::engine_presentation() else {
        return;
    };
    let mut runtime = UiRuntime::new(1);
    push_toast(
        &mut player_runtime,
        &mut runtime,
        1,
        "Welcome",
        "to the server",
    );
    push_toast(&mut player_runtime, &mut runtime, 2, "Second", "");
    let start = runtime.hud().toasts()[0].received_millis;
    let title_bottom = |presentation: &UiPresentationRuntime, wanted: &str| {
        text(presentation.toast_draw_nodes(), wanted).map(|node| node.dest.y + node.dest.h)
    };
    build(&player_runtime, &mut presentation, &runtime, start);
    // Wholly above the screen, the title is culled or drawn off the top edge.
    let hidden = title_bottom(&presentation, "Welcome");
    assert!(
        hidden.is_none_or(|bottom| bottom <= 0.0),
        "starts above the top edge: {hidden:?}"
    );
    build(&player_runtime, &mut presentation, &runtime, start + 1_000);
    let shown = title_bottom(&presentation, "Welcome").unwrap();
    assert!(shown > 0.0 && shown <= 32.0, "slid 32 px down: {shown}");
    assert!(text(presentation.toast_draw_nodes(), "to the server").is_some());
    // One at a time: the second waits for the first to slide away.
    assert!(text(presentation.toast_draw_nodes(), "Second").is_none());
    build(
        &player_runtime,
        &mut presentation,
        &runtime,
        start + 3_400 + 1_000,
    );
    assert!(text(presentation.toast_draw_nodes(), "Second").is_some());
    assert!(text(presentation.toast_draw_nodes(), "Welcome").is_none());
}

/// Local-only: writes `toast_screen.png` when `CINNABAR_FORM_SNAPSHOT_DIR` is set.
#[test]
fn toast_screen_snapshot() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) = super::engine_hud_tests::engine_presentation_with(
        super::super::forms::pack_harness::font(),
    ) else {
        return;
    };
    let mut runtime = UiRuntime::new(1);
    push_toast(
        &mut player_runtime,
        &mut runtime,
        1,
        "Welcome",
        "to the server",
    );
    let start = runtime.hud().toasts()[0].received_millis;
    let input = presentation
        .build(
            &player_runtime,
            &runtime,
            start + 1_000,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::forms::snapshot::write(&input, "toast_screen");
}
