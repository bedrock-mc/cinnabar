//! Admission behavior is exercised through physical input and captured packets in block-use tests.

use super::*;

#[test]
fn maintenance_reads_current_buttons_after_neutral_rearm() {
    let mut router = semantic_input::SemanticInputRouter::default();
    router
        .route(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_buttons: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let input = SemanticInputSnapshot(semantic_input::finalized::FinalizedInput::from_finalized(
        router.finalize().unwrap(),
    ));
    let mut runtime = InteractionRuntime::default();
    runtime.frames.quarantine();
    assert!(!held(Some(&runtime), &input, Action::Attack));
    runtime.frames.observe_neutral(true);
    assert!(held(Some(&runtime), &input, Action::Attack));
    assert!(!is_blocked(Some(&runtime)));
}
