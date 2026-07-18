use std::f32::consts::{PI, TAU};

use bevy::{
    anti_alias::fxaa::Fxaa,
    core_pipeline::tonemapping::Tonemapping,
    input::{mouse::AccumulatedMouseMotion, touch::Touches},
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use semantic_input::{Action, PerspectiveMode};
use sim::{Aabb, CollisionWorld, Vec3 as SimVec3};
use ui::UserSettings;

use crate::local_player::{
    CameraPose, InteractionOriginSnapshot, LocalAvatarPresentation, LocalViewPose,
};
use crate::semantic_controls::{
    SemanticInputRuntime, SemanticInputSnapshot, SemanticTouchTargets, finalize_semantic_input,
};
use crate::settings_runtime::RuntimeSettings;

pub const PITCH_LIMIT: f32 = 89.9_f32.to_radians();
pub const DEFAULT_HORIZONTAL_FOV_RADIANS: f32 = 90.0_f32.to_radians();
/// Radius declared by the pinned `minecraft:camera_orbit` vanilla presets.
pub const THIRD_PERSON_RADIUS_BLOCKS: f32 = 4.0;
pub const THIRD_PERSON_COLLISION_RADIUS_BLOCKS: f32 = 0.2;
const MIN_FOV_RADIANS: f32 = PI / 180.0;
const MAX_FOV_RADIANS: f32 = PI - MIN_FOV_RADIANS;
const DEFAULT_ASPECT_RATIO: f32 = 16.0 / 9.0;

pub const AUTO_FLY_PERIOD_SECONDS: f32 = 24.0;
pub const AUTO_FLY_MAX_HORIZONTAL_BLOCKS: f32 = 128.0;
const AUTO_FLY_RADIUS_BLOCKS: f32 = AUTO_FLY_MAX_HORIZONTAL_BLOCKS * 0.5;
const AUTO_FLY_VERTICAL_BLOCKS: f32 = 8.0;

/// Marks and configures the app's player-attached camera rig.
#[derive(Component, Debug, Clone, Copy)]
pub struct FlyCamera {
    pub speed: f32,
    pub look_sensitivity: Vec2,
}

/// Completes cursor, look, and movement updates before systems sample the
/// camera's final transform for the current frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlyCameraUpdateSet;

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            speed: 24.0,
            look_sensitivity: Vec2::splat(0.002),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraSettingsError {
    StaleGeneration { previous: u64, actual: u64 },
    NonFiniteFov,
    FovOutOfRange,
}

/// App-owned handoff from retained menu settings to the live camera.
///
/// Replacements are monotonic and atomic, so a stale UI frame or malformed
/// value cannot partially change the FOV or perspective.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CameraSettingsAuthority {
    generation: u64,
    horizontal_fov_degrees: f32,
    perspective: PerspectiveMode,
}

impl Default for CameraSettingsAuthority {
    fn default() -> Self {
        let settings = UserSettings::default();
        Self {
            generation: 0,
            horizontal_fov_degrees: settings.video.horizontal_fov_degrees,
            perspective: settings.gameplay.default_perspective,
        }
    }
}

impl CameraSettingsAuthority {
    pub fn replace(
        &mut self,
        generation: u64,
        settings: &UserSettings,
    ) -> Result<(), CameraSettingsError> {
        if generation <= self.generation {
            return Err(CameraSettingsError::StaleGeneration {
                previous: self.generation,
                actual: generation,
            });
        }
        let fov = settings.video.horizontal_fov_degrees;
        if !fov.is_finite() {
            return Err(CameraSettingsError::NonFiniteFov);
        }
        if !(30.0..=120.0).contains(&fov) {
            return Err(CameraSettingsError::FovOutOfRange);
        }
        self.generation = generation;
        self.horizontal_fov_degrees = fov;
        self.perspective = settings.gameplay.default_perspective;
        Ok(())
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn horizontal_fov_degrees(&self) -> f32 {
        self.horizontal_fov_degrees
    }

    #[must_use]
    pub const fn perspective(&self) -> PerspectiveMode {
        self.perspective
    }

    fn cycle_perspective(&mut self) {
        self.perspective = next_perspective(self.perspective);
    }

    pub(crate) fn reset_perspective(&mut self) {
        self.perspective = PerspectiveMode::FirstPerson;
    }
}

#[must_use]
pub const fn next_perspective(current: PerspectiveMode) -> PerspectiveMode {
    match current {
        PerspectiveMode::FirstPerson => PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonBack => PerspectiveMode::ThirdPersonFront,
        PerspectiveMode::ThirdPersonFront => PerspectiveMode::FirstPerson,
    }
}

#[must_use]
pub fn perspective_look_delta(delta: Vec2, perspective: PerspectiveMode) -> Vec2 {
    // The pinned third_person_front preset declares invert_x_input=true;
    // neither first person nor the rear orbit does.
    if perspective == PerspectiveMode::ThirdPersonFront {
        Vec2::new(-delta.x, delta.y)
    } else {
        delta
    }
}

/// Computes the unobstructed vanilla preset pose.
///
/// This function deliberately does not shorten the third-person boom: that
/// requires a world collision query and must be applied by a separate,
/// authoritative camera-avoidance stage rather than guessed here.
#[must_use]
pub fn perspective_pose(
    subject_translation: Vec3,
    subject_rotation: Quat,
    perspective: PerspectiveMode,
) -> Transform {
    let forward = subject_rotation * Vec3::NEG_Z;
    match perspective {
        PerspectiveMode::FirstPerson => Transform {
            translation: subject_translation,
            rotation: subject_rotation,
            ..default()
        },
        PerspectiveMode::ThirdPersonBack => {
            let translation = subject_translation - forward * THIRD_PERSON_RADIUS_BLOCKS;
            Transform {
                translation,
                rotation: subject_rotation,
                ..default()
            }
        }
        PerspectiveMode::ThirdPersonFront => {
            let horizontal_forward = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
            let horizontal_forward = if horizontal_forward == Vec3::ZERO {
                Vec3::NEG_Z
            } else {
                horizontal_forward
            };
            let translation = subject_translation + horizontal_forward * THIRD_PERSON_RADIUS_BLOCKS;
            Transform::from_translation(translation)
                .looking_at(subject_translation, subject_rotation * Vec3::Y)
        }
    }
}

/// Resolves the third-person camera boom against authoritative collision data.
///
/// The camera is represented by a radius-0.2 axis-aligned point sweep. Missing
/// collision data fails closed at the subject instead of allowing the camera
/// to pass through an unloaded column.
#[must_use]
pub fn collision_safe_perspective_pose(
    subject_translation: Vec3,
    subject_rotation: Quat,
    perspective: PerspectiveMode,
    world: &impl CollisionWorld,
) -> Transform {
    let mut pose = perspective_pose(subject_translation, subject_rotation, perspective);
    if perspective == PerspectiveMode::FirstPerson {
        return pose;
    }

    let delta = pose.translation - subject_translation;
    let origin = SimVec3::new(
        f64::from(subject_translation.x),
        f64::from(subject_translation.y),
        f64::from(subject_translation.z),
    );
    let sweep = SimVec3::new(f64::from(delta.x), f64::from(delta.y), f64::from(delta.z));
    let radius = f64::from(THIRD_PERSON_COLLISION_RADIUS_BLOCKS);
    let camera = Aabb::new(
        origin - SimVec3::new(radius, radius, radius),
        origin + SimVec3::new(radius, radius, radius),
    );
    let Ok(collisions) = world.collision_boxes(camera.swept(sweep)) else {
        pose.translation = subject_translation;
        return pose;
    };
    let fraction = collisions
        .value
        .into_iter()
        .filter_map(|collision| segment_entry_fraction(origin, sweep, collision.grown(radius)))
        .fold(1.0_f64, f64::min);
    pose.translation = subject_translation + delta * fraction as f32;
    pose
}

/// Fails closed when no collision world is available. A third-person boom is
/// never exposed through unloaded space; presentation remains at the eye until
/// authoritative collision data arrives.
#[must_use]
pub fn unavailable_world_perspective_pose(
    subject_translation: Vec3,
    subject_rotation: Quat,
    _perspective: PerspectiveMode,
) -> Transform {
    Transform {
        translation: subject_translation,
        rotation: subject_rotation,
        ..default()
    }
}

fn segment_entry_fraction(origin: SimVec3, delta: SimVec3, bounds: Aabb) -> Option<f64> {
    let mut entry = 0.0_f64;
    let mut exit = 1.0_f64;
    for axis in 0..3 {
        if delta[axis].abs() <= f64::EPSILON {
            if origin[axis] < bounds.min[axis] || origin[axis] > bounds.max[axis] {
                return None;
            }
            continue;
        }
        let first = (bounds.min[axis] - origin[axis]) / delta[axis];
        let second = (bounds.max[axis] - origin[axis]) / delta[axis];
        entry = entry.max(first.min(second));
        exit = exit.min(first.max(second));
        if entry > exit {
            return None;
        }
    }
    (exit >= 0.0 && entry <= 1.0).then_some(entry.clamp(0.0, 1.0))
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
            .init_resource::<Touches>()
            .insert_resource(AutoFly::new(self.auto_fly))
            .init_resource::<CameraSettingsAuthority>()
            .init_resource::<LocalViewPose>()
            .init_resource::<CameraPose>()
            .init_resource::<InteractionOriginSnapshot>()
            .init_resource::<LocalAvatarPresentation>()
            .init_resource::<SemanticInputRuntime>()
            .init_resource::<SemanticInputSnapshot>()
            .init_resource::<SemanticTouchTargets>()
            .init_resource::<RuntimeSettings>()
            .add_systems(Startup, spawn_fly_camera)
            .add_systems(Update, finalize_semantic_input.before(FlyCameraUpdateSet))
            .add_systems(
                Update,
                (
                    (apply_runtime_camera_settings, update_camera_fov)
                        .chain()
                        .before(FlyCameraUpdateSet),
                    (
                        update_cursor_capture,
                        update_perspective,
                        update_look,
                        update_movement,
                    )
                        .chain()
                        .in_set(FlyCameraUpdateSet),
                ),
            );
    }
}

/// Converts the user-facing horizontal FOV to Bevy's aspect-correct vertical
/// FOV while keeping malformed or zero-size window input finite and valid.
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

fn apply_runtime_camera_settings(
    runtime: Res<RuntimeSettings>,
    mut camera: ResMut<CameraSettingsAuthority>,
) {
    let (generation, settings) = runtime.user_settings_update();
    if generation > camera.generation() {
        let _ = camera.replace(generation, settings);
    }
}

fn spawn_fly_camera(
    mut commands: Commands,
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
) {
    let camera = FlyCamera::default();
    commands.spawn((
        Camera3d::default(),
        // Multisampled presentation is not portable: Depth32Float rejects some
        // sample counts on macOS, and Bevy/wgpu's DX12 resolve path presents a
        // black frame on affected adapters. FXAA retains edge smoothing without
        // a multisampled color/depth target or backend-specific resolve step.
        Msaa::Off,
        Fxaa::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: horizontal_fov_to_vertical(
                settings.horizontal_fov_degrees().to_radians(),
                window_aspect(&window),
            ),
            ..default()
        }),
        Tonemapping::None,
        camera,
        perspective_pose(
            view.eye_translation(),
            view.rotation(),
            settings.perspective(),
        ),
    ));
}

fn update_camera_fov(
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<CameraSettingsAuthority>,
    mut cameras: Query<&mut Projection, With<FlyCamera>>,
) {
    let vertical = horizontal_fov_to_vertical(
        settings.horizontal_fov_degrees().to_radians(),
        window_aspect(&window),
    );
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = vertical;
        }
    }
}

fn update_perspective(
    input: Res<SemanticInputSnapshot>,
    mut settings: ResMut<CameraSettingsAuthority>,
) {
    if !input.phase(Action::CyclePerspective).pressed {
        return;
    }
    settings.cycle_perspective();
}

#[cfg(test)]
pub(crate) fn movement_axes(keys: &ButtonInput<KeyCode>) -> Vec3 {
    let right = axis(keys.pressed(KeyCode::KeyD), keys.pressed(KeyCode::KeyA));
    let up = axis(
        keys.pressed(KeyCode::Space),
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
    );
    let forward = axis(keys.pressed(KeyCode::KeyW), keys.pressed(KeyCode::KeyS));
    Vec3::new(right, up, forward)
}

#[cfg(test)]
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
    input: Res<SemanticInputSnapshot>,
    settings: Res<CameraSettingsAuthority>,
    camera: Single<&FlyCamera>,
    mut view: ResMut<LocalViewPose>,
) {
    let look_delta = Vec2::from_array(input.look_delta());
    if look_delta == Vec2::ZERO {
        return;
    }

    let (yaw, pitch, roll) = view.rotation().to_euler(EulerRot::YXZ);
    let delta = perspective_look_delta(look_delta, settings.perspective());
    let (yaw, pitch) = look_angles(yaw, pitch, delta, camera.look_sensitivity);
    view.set_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll));
}

fn update_movement(
    input: Res<SemanticInputSnapshot>,
    time: Res<Time>,
    mut auto_fly: ResMut<AutoFly>,
    local_physics: Option<Res<crate::movement::LocalPhysicsController>>,
    camera: Single<&FlyCamera>,
    mut view: ResMut<LocalViewPose>,
) {
    if auto_fly.enabled() {
        let externally_moved = auto_fly
            .last_path_position
            .is_some_and(|last| last.distance_squared(view.eye_translation()) > 0.01);
        if externally_moved || auto_fly.path_anchor.is_none() {
            auto_fly.path_anchor = Some(view.eye_translation());
            auto_fly.elapsed_seconds = 0.0;
        }
        auto_fly.elapsed_seconds =
            (auto_fly.elapsed_seconds + time.delta_secs()).rem_euclid(AUTO_FLY_PERIOD_SECONDS);
        let next = auto_fly.path_anchor.expect("auto-fly anchor initialized")
            + auto_fly_offset(auto_fly.elapsed_seconds);
        view.set_eye_translation(next);
        if let Some(target) = auto_fly.look_target {
            view.set_rotation(look_at_target(next, target));
        }
        auto_fly.last_path_position = Some(next);
        return;
    }

    if local_physics.is_some_and(|physics| physics.is_active()) {
        return;
    }

    let movement = input.movement();
    let axes = Vec3::new(
        movement[0],
        f32::from(u8::from(input.phase(Action::Jump).held))
            - f32::from(u8::from(input.phase(Action::Sneak).held)),
        movement[1],
    );
    let axes = axes.normalize_or_zero();
    if axes == Vec3::ZERO {
        return;
    }

    let (yaw, _, _) = view.rotation().to_euler(EulerRot::YXZ);
    let yaw_rotation = Quat::from_rotation_y(yaw);
    let right = yaw_rotation * Vec3::X;
    let forward = yaw_rotation * Vec3::NEG_Z;
    let direction = (right * axes.x + Vec3::Y * axes.y + forward * axes.z).normalize_or_zero();
    let next = view.eye_translation() + direction * camera.speed * time.delta_secs();
    view.set_eye_translation(next);
}

#[cfg(test)]
mod tests;
