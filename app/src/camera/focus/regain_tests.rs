use super::{recapture_tests::assert_centred_capture, tests::focus_app, *};
use client_shell::ui_resources::UiRuntimeResource;

#[test]
fn gameplay_capture_returns_without_click_and_discards_reentry_motion() {
    let (mut app, window) = focus_app();
    app.update();
    for _ in 0..2 {
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        // Background frames must not replace the retained gameplay capture request.
        app.update();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(None);
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(30.0, -12.0);

        app.update();

        assert_centred_capture(&app, window);
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .get_pressed()
                .next()
                .is_none()
        );
    }
}

#[test]
fn screen_opened_while_unfocused_keeps_the_pointer_on_return() {
    let (mut app, window) = focus_app();
    app.update();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    app.update();
    app.world_mut()
        .resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
            world
                .resource_mut::<UiRuntimeResource>()
                .open_chat(&mut player);
        });
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;

    app.update();

    assert!(app.world().resource::<UiRuntimeResource>().chat_focused());
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
}
