//! Admission behavior is exercised through physical input and captured packets in block-use tests.

use super::*;

#[test]
fn maintenance_uses_admitted_buttons_until_the_cursor_is_empty() {
    let mut runtime = InteractionRuntime::default();
    runtime.queue.quarantine();
    assert!(!held(Some(&runtime), Action::Attack, true));
    runtime.queue.observe_neutral(true);
    assert!(held(Some(&runtime), Action::Attack, true));
    assert!(!is_blocked(Some(&runtime)));
}
