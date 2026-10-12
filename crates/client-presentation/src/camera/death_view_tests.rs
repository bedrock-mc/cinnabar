use super::*;
use semantic_input::{
    Action, ActionSnapshot, DeviceFrame, KeyboardMouseFrame, PerspectiveMode, SemanticInputRouter,
};

/// The routed semantic actions for the next frame.
#[derive(Resource)]
struct Routed(Option<ActionSnapshot>);

/// Whether the local player is dead.
#[derive(Resource)]
struct Dead(bool);

/// The local body's rotation.
#[derive(Resource)]
struct Body(Quat);

/// Runs the perspective update from the fixture resources.
fn drive(
    routed: Res<Routed>,
    dead: Res<Dead>,
    body: Res<Body>,
    settings: ResMut<CameraSettingsAuthority>,
) {
    update_perspective(
        crate::observations::InputObservation(routed.0.as_ref()),
        dead.0,
        body.0,
        settings,
    );
}

/// An app that updates `settings` through the perspective system each frame.
fn app(settings: CameraSettingsAuthority) -> App {
    let mut app = App::new();
    app.insert_resource(settings)
        .insert_resource(Routed(None))
        .insert_resource(Dead(false))
        .insert_resource(Body(Quat::IDENTITY))
        .add_systems(Update, drive);
    app
}

/// The perspective the camera presents.
fn perspective(app: &App) -> PerspectiveMode {
    app.world()
        .resource::<CameraSettingsAuthority>()
        .perspective()
}

/// Camera settings with the Java 1.7 animation mode on.
fn java_settings() -> CameraSettingsAuthority {
    let mut configured = ui::UserSettings::default();
    configured.video.java_animations = true;
    let mut settings = CameraSettingsAuthority::default();
    settings.replace(1, &configured).unwrap();
    assert!(settings.feel().java_animations);
    settings
}

#[test]
fn dying_in_first_person_shows_the_body_from_behind_until_respawn() {
    let mut app = app(CameraSettingsAuthority::default());
    app.update();
    assert_eq!(perspective(&app), PerspectiveMode::FirstPerson);

    app.insert_resource(Dead(true));
    app.update();
    assert_eq!(perspective(&app), PerspectiveMode::ThirdPersonBack);

    app.insert_resource(Dead(false));
    app.update();
    assert_eq!(perspective(&app), PerspectiveMode::FirstPerson);
}

#[test]
fn dying_keeps_a_chosen_front_view() {
    let mut settings = CameraSettingsAuthority::default();
    settings.cycle_perspective();
    settings.cycle_perspective();
    let mut app = app(settings);
    app.insert_resource(Dead(true));
    app.update();
    assert_eq!(perspective(&app), PerspectiveMode::ThirdPersonFront);
}

#[test]
fn perspective_hotkey_is_ignored_while_dead() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                keys: vec![0x3e],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert!(snapshot.phases[Action::CyclePerspective as usize].pressed);
    let mut app = app(CameraSettingsAuthority::default());
    app.insert_resource(Dead(true))
        .insert_resource(Routed(Some(snapshot)));
    app.update();

    app.insert_resource(Dead(false))
        .insert_resource(Routed(None));
    app.update();
    assert_eq!(perspective(&app), PerspectiveMode::FirstPerson);
}

#[test]
fn java_animation_mode_keeps_first_person_on_death() {
    let mut app = app(java_settings());
    app.insert_resource(Dead(true));
    app.update();
    assert_eq!(perspective(&app), PerspectiveMode::FirstPerson);
}

#[test]
fn session_reset_clears_the_death_view() {
    let mut settings = CameraSettingsAuthority::default();
    settings.set_death_view(Some(Quat::IDENTITY));
    assert_eq!(settings.perspective(), PerspectiveMode::ThirdPersonBack);
    settings.reset_perspective();
    assert_eq!(settings.perspective(), PerspectiveMode::FirstPerson);
}

#[test]
fn the_death_camera_faces_level_along_the_body_and_ignores_later_turns() {
    let mut app = app(CameraSettingsAuthority::default());
    let body = Quat::from_euler(EulerRot::YXZ, 1.2, -0.9, 0.0);
    app.insert_resource(Dead(true)).insert_resource(Body(body));
    app.update();
    let locked = app
        .world()
        .resource::<CameraSettingsAuthority>()
        .death_rotation()
        .unwrap();
    assert!(locked.angle_between(Quat::from_rotation_y(1.2)) < 1e-5);

    app.insert_resource(Body(Quat::from_rotation_y(-2.0)));
    app.update();
    assert_eq!(
        app.world()
            .resource::<CameraSettingsAuthority>()
            .death_rotation(),
        Some(locked)
    );
}
