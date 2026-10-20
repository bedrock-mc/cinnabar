use super::CursorFocus;

#[test]
fn focus_return_waits_for_click_and_a_loss_batch_consumes_the_click() {
    let mut focus = CursorFocus::default();
    focus.begin_frame(true);
    assert!(focus.allow_capture(false, false));
    focus.focus_changed(false);
    focus.focus_changed(true);
    assert!(!focus.available());
    assert!(!focus.allow_capture(false, true));
    focus.begin_frame(true);
    assert!(focus.available());
    assert!(!focus.allow_capture(false, false));
    assert!(focus.allow_capture(false, true));
}

#[test]
fn occlusion_survives_focus_gain_and_releases_only_on_explicit_return() {
    let mut focus = CursorFocus::default();
    focus.occlusion_changed(true);
    focus.begin_frame(true);
    assert!(!focus.allow_capture(false, true));
    focus.occlusion_changed(false);
    assert!(!focus.allow_capture(false, false));
    assert!(focus.allow_capture(false, true));
}

#[test]
fn pause_screen_return_authorizes_capture_but_focus_regain_does_not() {
    let mut focus = CursorFocus::default();
    focus.begin_frame(false);
    assert!(!focus.allow_capture(true, false));
    focus.begin_frame(true);
    assert!(!focus.allow_capture(true, false));
    focus.record_activation(true);
    focus.authorize_screen_return();
    assert!(focus.allow_capture(false, false));
}

#[test]
fn programmatic_screen_close_after_focus_return_does_not_rearm_capture() {
    let mut focus = CursorFocus::default();
    focus.begin_frame(false);
    assert!(!focus.allow_capture(true, false));
    focus.begin_frame(true);
    assert!(!focus.allow_capture(true, false));
    focus.record_activation(true);
    assert!(!focus.allow_capture(false, false));
    assert!(focus.allow_capture(false, true));
}

#[test]
fn delayed_screen_return_retains_explicit_dismissal_until_transport_finishes() {
    let mut focus = CursorFocus::default();
    focus.begin_frame(false);
    assert!(!focus.allow_capture(true, false));
    focus.begin_frame(true);
    focus.record_activation(true);
    focus.authorize_screen_return();
    assert!(focus.allow_capture(true, false));
    focus.begin_frame(true);
    assert!(focus.allow_capture(false, false));
    focus.begin_frame(false);
    focus.begin_frame(true);
    assert!(!focus.allow_capture(false, false));
}

#[test]
fn programmatic_screen_return_cannot_authorize_delayed_capture() {
    let mut focus = CursorFocus::default();
    focus.begin_frame(false);
    assert!(!focus.allow_capture(true, false));
    focus.begin_frame(true);
    focus.authorize_screen_return();
    assert!(!focus.allow_capture(true, false));
    focus.begin_frame(true);
    assert!(!focus.allow_capture(false, false));
}

/// Grab-free input resumes only after focus returns and gameplay requests capture again.
#[test]
fn focus_loss_ends_grab_free_capture() {
    let mut focus = CursorFocus::default();
    focus.set_grab_free_capture(true);
    assert!(focus.grab_free_capture());
    focus.begin_frame(false);
    assert!(!focus.grab_free_capture());
    focus.begin_frame(true);
    assert!(!focus.grab_free_capture());
    focus.set_grab_free_capture(true);
    assert!(focus.grab_free_capture());
}

#[test]
fn captured_gameplay_resumes_after_focus_and_occlusion_return() {
    for occluded in [false, true] {
        let mut focus = CursorFocus::default();
        focus.record_capture(true);
        focus.begin_frame(false);
        focus.occlusion_changed(occluded);
        assert!(!focus.capture_allowed());
        // Repeated background frames must retain the original capture state.
        focus.record_capture(false);
        focus.begin_frame(false);
        focus.begin_frame(true);
        assert_eq!(focus.capture_allowed(), !occluded);
        focus.occlusion_changed(false);
        assert!(focus.capture_allowed());
    }
}

#[test]
fn same_batch_focus_loss_discards_input_before_capture_resumes() {
    let mut focus = CursorFocus::default();
    focus.record_capture(true);
    focus.focus_changed(false);
    focus.focus_changed(true);
    assert!(!focus.capture_allowed());
    focus.record_capture(false);
    focus.begin_frame(true);
    assert!(focus.capture_allowed());
}

#[test]
fn released_pointer_is_not_restored_after_focus_return() {
    let mut focus = CursorFocus::default();
    focus.record_capture(true);
    focus.record_capture(false);
    focus.begin_frame(false);
    focus.begin_frame(true);
    assert!(!focus.capture_allowed());
    assert!(focus.allow_capture(false, true));
}
