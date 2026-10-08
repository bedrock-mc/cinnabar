use super::*;
use bevy::{camera::CameraProjection, window::WindowResolution};

fn spawned_projection() -> PerspectiveProjection {
    let mut settings = UserSettings::default();
    settings.video.horizontal_fov_degrees = 110.0;
    let mut authority = CameraSettingsAuthority::default();
    authority.replace(1, &settings).unwrap();
    let mut app = App::new();
    app.insert_resource(authority)
        .init_resource::<LocalViewPose>()
        .init_resource::<antialiasing::CameraAntiAliasingSupport>()
        .add_systems(Startup, spawn_fly_camera);
    app.world_mut().spawn((
        Window {
            resolution: WindowResolution::new(1980, 1080),
            ..default()
        },
        PrimaryWindow,
    ));
    app.update();
    let projection = app
        .world_mut()
        .query_filtered::<&Projection, With<FlyCamera>>()
        .single(app.world())
        .unwrap();
    let Projection::Perspective(perspective) = projection else {
        panic!("spawned camera must use a perspective projection");
    };
    perspective.clone()
}

#[test]
fn world_near_plane_stays_outside_a_touching_wall_at_sprint_fov() {
    let mut projection = spawned_projection();
    projection.fov = projection_fov_radians(128.0);
    let view_from_clip = projection.get_clip_from_view().inverse();
    let wall_clearance = sim::PLAYER_WIDTH as f32 * 0.5;
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            let near_corner = view_from_clip.project_point3(Vec3::new(x, y, 1.0));
            assert!(near_corner.is_finite());
            assert!(
                near_corner.x.abs() < wall_clearance,
                "near-plane corner {near_corner:?} enters the wall outside the player collision box"
            );
        }
    }
}

#[test]
fn world_projection_preserves_geometry_close_to_the_eye() {
    let projection = spawned_projection();
    let depth = projection
        .get_clip_from_view()
        .project_point3(Vec3::new(0.0, 0.0, -0.05))
        .z;
    assert!(
        (0.0..=1.0).contains(&depth),
        "nearby geometry is clipped with reverse-Z depth {depth}"
    );
}
