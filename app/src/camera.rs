use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};

pub const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
pub const DEFAULT_HORIZONTAL_FOV_RADIANS: f32 = 2.0 * PI / 3.0;
const DEFAULT_ASPECT_RATIO: f32 = 16.0 / 9.0;
const MIN_FOV_RADIANS: f32 = PI / 180.0;
const MAX_FOV_RADIANS: f32 = PI - MIN_FOV_RADIANS;

pub const AUTO_FLY_PERIOD_SECONDS: f32 = 24.0;
pub const AUTO_FLY_MAX_HORIZONTAL_BLOCKS: f32 = 128.0;
const AUTO_FLY_RADIUS_BLOCKS: f32 = AUTO_FLY_MAX_HORIZONTAL_BLOCKS * 0.5;
const AUTO_FLY_VERTICAL_BLOCKS: f32 = 8.0;

/// Marks and configures the app's first-person camera.
#[derive(Component, Debug, Clone, Copy)]
pub struct FlyCamera {
    pub speed: f32,
    pub look_sensitivity: Vec2,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            speed: 24.0,
            look_sensitivity: Vec2::splat(0.002),
        }
    }
}

/// Enables deterministic camera movement for `--auto-fly` acceptance runs.
#[derive(Resource, Debug, Clone, Copy)]
pub struct AutoFly {
    enabled: bool,
    capture_pending: bool,
    path_anchor: Option<Vec3>,
    last_path_position: Option<Vec3>,
    look_target: Option<Vec3>,
    elapsed_seconds: f32,
}

impl AutoFly {
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            capture_pending: enabled,
            path_anchor: None,
            last_path_position: None,
            look_target: None,
            elapsed_seconds: 0.0,
        }
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_look_target(&mut self, target: Vec3) {
        self.look_target = Some(target);
    }
}

#[must_use]
pub fn auto_fly_offset(seconds: f32) -> Vec3 {
    let phase = seconds.rem_euclid(AUTO_FLY_PERIOD_SECONDS) / AUTO_FLY_PERIOD_SECONDS;
    let angle = phase * TAU;
    Vec3::new(
        AUTO_FLY_RADIUS_BLOCKS * (angle.cos() - 1.0),
        AUTO_FLY_VERTICAL_BLOCKS * (angle * 2.0).sin(),
        AUTO_FLY_RADIUS_BLOCKS * angle.sin(),
    )
}

#[must_use]
pub fn look_at_target(position: Vec3, target: Vec3) -> Quat {
    if position.distance_squared(target) <= f32::EPSILON {
        return Quat::IDENTITY;
    }
    Transform::from_translation(position)
        .looking_at(target, Vec3::Y)
        .rotation
}

/// Spawns and drives one [`Camera3d`] fly camera.
pub struct FlyCameraPlugin {
    auto_fly: bool,
}

impl FlyCameraPlugin {
    #[must_use]
    pub const fn new(auto_fly: bool) -> Self {
        Self { auto_fly }
    }
}

impl Default for FlyCameraPlugin {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .insert_resource(AutoFly::new(self.auto_fly))
            .add_systems(Startup, spawn_fly_camera)
            .add_systems(
                Update,
                (
                    update_camera_fov,
                    (update_cursor_capture, update_look, update_movement).chain(),
                ),
            );
    }
}

/// Converts a horizontal field of view to Bevy's vertical field of view while
/// keeping malformed or transient zero-size window inputs finite and valid.
#[must_use]
pub fn horizontal_fov_to_vertical(horizontal: f32, aspect: f32) -> f32 {
    let horizontal = if horizontal.is_finite() {
        horizontal.clamp(MIN_FOV_RADIANS, MAX_FOV_RADIANS)
    } else {
        DEFAULT_HORIZONTAL_FOV_RADIANS
    };
    let aspect = if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        DEFAULT_ASPECT_RATIO
    };
    (2.0 * ((horizontal * 0.5).tan() / aspect).atan()).clamp(MIN_FOV_RADIANS, MAX_FOV_RADIANS)
}

fn window_aspect(window: &Window) -> f32 {
    window.resolution.width() / window.resolution.height()
}

fn spawn_fly_camera(mut commands: Commands, window: Single<&Window, With<PrimaryWindow>>) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Sample8,
        Projection::Perspective(PerspectiveProjection {
            fov: horizontal_fov_to_vertical(DEFAULT_HORIZONTAL_FOV_RADIANS, window_aspect(&window)),
            ..default()
        }),
        Tonemapping::None,
        FlyCamera::default(),
        Transform::from_xyz(0.0, 80.0, 0.0),
    ));
}

fn update_camera_fov(
    window: Single<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Projection, With<FlyCamera>>,
) {
    let vertical =
        horizontal_fov_to_vertical(DEFAULT_HORIZONTAL_FOV_RADIANS, window_aspect(&window));
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = vertical;
        }
    }
}

pub(crate) fn movement_axes(keys: &ButtonInput<KeyCode>) -> Vec3 {
    let right = axis(keys.pressed(KeyCode::KeyD), keys.pressed(KeyCode::KeyA));
    let up = axis(
        keys.pressed(KeyCode::Space),
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    );
    let forward = axis(keys.pressed(KeyCode::KeyW), keys.pressed(KeyCode::KeyS));
    Vec3::new(right, up, forward)
}

fn axis(positive: bool, negative: bool) -> f32 {
    f32::from(u8::from(positive)) - f32::from(u8::from(negative))
}

pub(crate) fn look_angles(
    yaw: f32,
    pitch: f32,
    mouse_delta: Vec2,
    sensitivity: Vec2,
) -> (f32, f32) {
    let yaw = yaw - mouse_delta.x * sensitivity.x;
    let pitch = (pitch - mouse_delta.y * sensitivity.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    (yaw, pitch)
}

pub(crate) fn input_is_active(window: &Window, cursor: &CursorOptions) -> bool {
    window.focused && cursor.grab_mode == CursorGrabMode::Locked && !cursor.visible
}

fn capture_cursor(cursor: &mut CursorOptions) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

fn release_cursor(cursor: &mut CursorOptions) {
    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
}

fn clear_controller_input(
    keys: &mut ButtonInput<KeyCode>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    mouse_motion: &mut AccumulatedMouseMotion,
) {
    keys.reset_all();
    mouse_buttons.reset_all();
    mouse_motion.delta = Vec2::ZERO;
}

pub(crate) fn update_cursor_capture(
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut auto_fly: ResMut<AutoFly>,
) {
    let (window, mut cursor) = window.into_inner();

    // Focus loss has priority over every capture request, including auto-fly.
    if !window.focused {
        release_cursor(&mut cursor);
        clear_controller_input(&mut keys, &mut mouse_buttons, &mut mouse_motion);
        auto_fly.capture_pending = false;
        return;
    }

    // Escape also wins if it arrives in the same frame as a left click.
    if keys.just_pressed(KeyCode::Escape) {
        release_cursor(&mut cursor);
        clear_controller_input(&mut keys, &mut mouse_buttons, &mut mouse_motion);
        auto_fly.capture_pending = false;
        return;
    }

    if mouse_buttons.just_pressed(MouseButton::Left) || auto_fly.capture_pending {
        capture_cursor(&mut cursor);
        auto_fly.capture_pending = false;
    }
}

fn update_look(
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut cameras: Query<(&mut Transform, &FlyCamera)>,
) {
    let (window, cursor) = window.into_inner();
    if !input_is_active(window, cursor) || mouse_motion.delta == Vec2::ZERO {
        return;
    }

    for (mut transform, camera) in &mut cameras {
        let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
        let (yaw, pitch) = look_angles(yaw, pitch, mouse_motion.delta, camera.look_sensitivity);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
    }
}

fn update_movement(
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut auto_fly: ResMut<AutoFly>,
    mut cameras: Query<(&mut Transform, &FlyCamera)>,
) {
    let (window, cursor) = window.into_inner();
    if auto_fly.enabled() {
        for (mut transform, _) in &mut cameras {
            let externally_moved = auto_fly
                .last_path_position
                .is_some_and(|last| last.distance_squared(transform.translation) > 0.01);
            if externally_moved || auto_fly.path_anchor.is_none() {
                auto_fly.path_anchor = Some(transform.translation);
                auto_fly.elapsed_seconds = 0.0;
            }
            auto_fly.elapsed_seconds =
                (auto_fly.elapsed_seconds + time.delta_secs()).rem_euclid(AUTO_FLY_PERIOD_SECONDS);
            let next = auto_fly.path_anchor.expect("auto-fly anchor initialized")
                + auto_fly_offset(auto_fly.elapsed_seconds);
            transform.translation = next;
            if let Some(target) = auto_fly.look_target {
                transform.rotation = look_at_target(next, target);
            }
            auto_fly.last_path_position = Some(next);
        }
        return;
    }

    if !input_is_active(window, cursor) {
        return;
    }

    let axes = movement_axes(&keys);
    let axes = axes.normalize_or_zero();
    if axes == Vec3::ZERO {
        return;
    }

    for (mut transform, camera) in &mut cameras {
        let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
        let yaw_rotation = Quat::from_rotation_y(yaw);
        let right = yaw_rotation * Vec3::X;
        let forward = yaw_rotation * Vec3::NEG_Z;
        let direction = (right * axes.x + Vec3::Y * axes.y + forward * axes.z).normalize_or_zero();
        transform.translation += direction * camera.speed * time.delta_secs();
    }
}
