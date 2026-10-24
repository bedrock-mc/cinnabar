use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};

use super::install_grab_free_cursor;

#[derive(Resource, Default)]
struct NativeCursorRequests(usize);

#[derive(Resource)]
struct Requested(CursorGrabMode);

/// Re-asserts the cursor every frame the way a screen that owns input does.
fn reassert_cursor(requested: Res<Requested>, mut cursors: Query<&mut CursorOptions>) {
    for mut cursor in &mut cursors {
        cursor.grab_mode = requested.0;
        cursor.visible = requested.0 == CursorGrabMode::None;
    }
}

/// Counts the cursor changes Winit's `Last` system would turn into native grab attempts.
fn count_native_requests(
    cursors: Query<(), Changed<CursorOptions>>,
    mut requests: ResMut<NativeCursorRequests>,
) {
    requests.0 += cursors.iter().count();
}

/// Builds a windowed app whose screen requests `requested` every frame, optionally grab-free.
fn cursor_app(requested: CursorGrabMode, grab_free: bool) -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<NativeCursorRequests>()
        .insert_resource(Requested(requested))
        .add_systems(Update, reassert_cursor)
        .add_systems(Last, count_native_requests);
    if grab_free {
        install_grab_free_cursor(&mut app);
    }
    let window = app
        .world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow))
        .id();
    (app, window)
}

#[test]
fn grab_free_platforms_send_no_native_cursor_requests() {
    let frames = 5;
    let (mut control, _) = cursor_app(CursorGrabMode::None, false);
    for _ in 0..frames {
        control.update();
    }
    assert_eq!(
        control.world().resource::<NativeCursorRequests>().0,
        frames,
        "without the guard every re-asserted release reaches the window backend"
    );

    let (mut app, window) = cursor_app(CursorGrabMode::None, true);
    for _ in 0..frames {
        app.update();
    }
    assert_eq!(app.world().resource::<NativeCursorRequests>().0, 0);
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
}

#[test]
fn grab_free_platforms_keep_a_requested_lock_free() {
    let (mut app, window) = cursor_app(CursorGrabMode::Locked, true);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<NativeCursorRequests>().0, 0);
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
}
