use super::*;

#[derive(Resource, Default)]
struct CursorNotifications(usize);

/// Observes the same change filter that Bevy uses before calling the windowing OS.
fn observe_cursor_changes(
    cursors: Query<(), Changed<CursorOptions>>,
    mut notifications: ResMut<CursorNotifications>,
) {
    notifications.0 += cursors.iter().count();
}

#[test]
fn idle_released_cursor_does_not_repeat_os_notifications() {
    let (mut app, window) = capture_test_app(false, CursorGrabMode::Locked, false, false);
    app.init_resource::<CursorNotifications>()
        .add_systems(PostUpdate, observe_cursor_changes);
    app.world_mut()
        .get_mut::<CursorOptions>(window)
        .unwrap()
        .hit_test = false;

    app.update();
    assert_eq!(app.world().resource::<CursorNotifications>().0, 1);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<CursorNotifications>().0,
        1,
        "an unchanged released cursor must not repeatedly call the OS"
    );

    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(app.world().resource::<CursorNotifications>().0, 2);
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible && !cursor.hit_test);

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert_eq!(app.world().resource::<CursorNotifications>().0, 3);
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible && !cursor.hit_test);

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert_eq!(app.world().resource::<CursorNotifications>().0, 3);
}
