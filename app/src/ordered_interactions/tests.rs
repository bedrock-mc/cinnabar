//! Admission behavior is exercised through physical input and captured packets in block-use tests.

use super::*;

#[test]
fn maintenance_reads_current_buttons_after_neutral_rearm() {
    let mut runtime = InteractionRuntime::default();
    runtime.frames.quarantine();
    assert!(!held(Some(&runtime), Action::Attack, true));
    runtime.frames.observe_neutral(true);
    assert!(held(Some(&runtime), Action::Attack, true));
    assert!(!is_blocked(Some(&runtime)));
}
