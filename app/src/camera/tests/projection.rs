use super::*;
use bevy::camera::CameraProjection;

/// Builds the app camera adapter with a primary window of the requested size.
fn camera_app(width: u32, height: u32) -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default());
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: WindowResolution::new(width, height),
                focused: true,
                ..default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.update();
    (app, window)
}

/// Reads the live fly-camera projection after the scheduled presentation update.
fn projection(app: &mut App) -> PerspectiveProjection {
    let projection = app
        .world_mut()
        .query_filtered::<&Projection, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    let Projection::Perspective(perspective) = projection else {
        panic!("fly camera projection is not perspective");
    };
    perspective.clone()
}

#[test]
fn native_full_viewport_fov_sets_vertical_angle_and_aspect_only_scales_horizontal_axis() {
    // Vanilla getFov uses normalized viewport fractions; bx::mtxProjRh
    // stores cot(FOV/2) on Y and cot(FOV/2)/aspect on X.
    for degrees in [30.0_f32, 60.0, 90.0, 110.0, 120.0] {
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0, 9.0 / 16.0] {
            let projection = PerspectiveProjection {
                fov: camera::projection_fov_radians(degrees),
                aspect_ratio: aspect,
                ..default()
            };
            let clip = projection.get_clip_from_view();
            let y_scale = 1.0 / (degrees.to_radians() * 0.5).tan();
            assert!((projection.fov - degrees.to_radians()).abs() < 1.0e-6);
            assert!((clip.y_axis.y - y_scale).abs() < 1.0e-6);
            assert!((clip.x_axis.x - y_scale / aspect).abs() < 1.0e-6);
        }
    }
}

#[test]
fn replacing_user_settings_preserves_110_degree_vertical_fov() {
    let (mut app, _) = camera_app(1600, 900);
    let mut settings = UserSettings::default();
    settings.video.horizontal_fov_degrees = 110.0;
    settings.video.fov_effects_scale = 0.0;
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(settings);
    app.update();

    let perspective = projection(&mut app);
    assert_eq!(
        app.world()
            .resource::<CameraSettingsAuthority>()
            .generation(),
        1
    );
    assert!((perspective.fov - 110.0_f32.to_radians()).abs() < 1.0e-6);
    let horizontal = 2.0 * (1.0 / perspective.get_clip_from_view().x_axis.x).atan();
    assert!((horizontal.to_degrees() - 137.0).abs() < 0.2);
}

#[test]
fn projection_fov_is_finite_and_bounded_for_bad_inputs() {
    for degrees in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 999.0] {
        let vertical = camera::projection_fov_radians(degrees);
        assert!(vertical.is_finite());
        assert!(vertical > 0.0 && vertical < std::f32::consts::PI);
    }
}

#[test]
fn plugin_spawns_camera_with_configured_default_vertical_fov() {
    let (mut app, _) = camera_app(1600, 900);
    let expected = UserSettings::default().video.horizontal_fov_degrees;
    assert!((projection(&mut app).fov - expected.to_radians()).abs() <= 1.0e-6);
}

#[test]
fn resizing_the_window_preserves_vertical_fov_and_updates_horizontal_projection() {
    let (mut app, window) = camera_app(1600, 900);
    let before = projection(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1200, 900);
    app.update();
    let after = projection(&mut app);

    assert_eq!(after.fov, before.fov);
    assert!((before.aspect_ratio - 16.0 / 9.0).abs() < 1.0e-6);
    assert!((after.aspect_ratio - 4.0 / 3.0).abs() < 1.0e-6);
    let before_clip = before.get_clip_from_view();
    let after_clip = after.get_clip_from_view();
    assert_eq!(before_clip.y_axis.y, after_clip.y_axis.y);
    assert!(after_clip.x_axis.x > before_clip.x_axis.x);
}

#[test]
fn zero_height_window_preserves_fov_with_a_finite_projection() {
    let (mut app, window) = camera_app(1600, 900);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1600, 0);
    app.update();

    let perspective = projection(&mut app);
    let expected_fov = UserSettings::default()
        .video
        .horizontal_fov_degrees
        .to_radians();
    assert!((perspective.fov - expected_fov).abs() < 1.0e-6);
    assert!(perspective.aspect_ratio.is_finite() && perspective.aspect_ratio > 0.0);
    assert!(perspective.get_clip_from_view().is_finite());
}
