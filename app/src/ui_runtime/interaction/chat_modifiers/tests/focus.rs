//! Inventory modifiers stay in step with the physical keys across focus loss and other
//! keyboard owners.

use super::*;

#[test]
fn inventory_modifiers_release_while_unfocused() {
    let mut h = Harness::new();
    h.press(KeyCode::ShiftLeft, None);
    h.press(KeyCode::ControlRight, None);
    assert_eq!(h.inventory_modifiers(), (true, true));
    h.set_focused(false);
    h.release(KeyCode::ShiftLeft);
    h.release(KeyCode::ControlRight);
    h.set_focused(true);
    h.app.update();
    assert_eq!(h.inventory_modifiers(), (false, false));
}

#[test]
fn inventory_modifiers_follow_edges_while_another_screen_owns_the_keyboard() {
    let mut h = Harness::new();
    h.press(KeyCode::ShiftLeft, None);
    h.app
        .insert_resource(client_state::EmoteInputConsumed(true));
    h.release(KeyCode::ShiftLeft);
    h.press(KeyCode::ControlLeft, None);
    h.app
        .world_mut()
        .remove_resource::<client_state::EmoteInputConsumed>();
    h.app.update();
    assert_eq!(h.inventory_modifiers(), (false, true));
}

#[test]
fn inventory_modifiers_release_on_focus_loss_without_release_events() {
    // Alt-tab: the window loses focus while keys are held and never sees their releases.
    let mut h = Harness::new();
    h.press(KeyCode::ShiftLeft, None);
    h.press(KeyCode::ControlLeft, None);
    assert_eq!(h.inventory_modifiers(), (true, true));
    h.set_focused(false);
    h.app.update();
    h.set_focused(true);
    h.app.update();
    assert_eq!(h.inventory_modifiers(), (false, false));
}
