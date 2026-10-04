use std::f32::consts::{PI, TAU};

use bevy::{
    anti_alias::fxaa::Fxaa,
    core_pipeline::tonemapping::Tonemapping,
    input::{
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
        touch::Touches,
    },
    log::debug,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use semantic_input::{Action, PerspectiveMode};
use sim::{Aabb, CollisionWorld, LenientCollisionBoxes, LenientSkipCounts, Vec3 as SimVec3};
use ui::UserSettings;

use crate::local_player::{
    CameraPose, InteractionOriginSnapshot, LocalAvatarPresentation, LocalAvatarVisibilityCarrier,
    LocalPlayerFrameCarrier, LocalViewPose,
};

mod bob;
mod easing;
pub mod facts;
pub mod fov;
mod hurt;
pub mod look;
mod overlay;
pub mod overlay_publish;
pub mod presentation;
mod server_view;
mod shake;

pub use bob::{HandSwayState, ViewEffect, WalkBobState, walk_bob_effect};
pub use fov::{CameraFovInputs, CameraFovState, SPYGLASS_FOV_MODIFIER};
pub use hurt::{CameraHurtState, LocalHurtEvent};
pub use overlay::{
    HeadMedium, OverlayKind, OverlayLayer, PortalProgress, ScreenEffectInputs, ScreenOverlays,
    VisionEffects, compute_overlays,
};
pub use presentation::{FirstPersonHandMotion, ScreenEffectFacts};
pub use server_view::{ServerCameraSkips, ServerCameraView};

pub const PITCH_LIMIT: f32 = 89.9_f32.to_radians();
/// Radius declared by the pinned `minecraft:camera_orbit` vanilla presets.
pub const THIRD_PERSON_RADIUS_BLOCKS: f32 = 4.0;
pub const THIRD_PERSON_COLLISION_RADIUS_BLOCKS: f32 = 0.2;
pub const THIRD_PERSON_COLLISION_EPSILON_BLOCKS: f32 = 0.001;
const _: () = assert!(THIRD_PERSON_COLLISION_EPSILON_BLOCKS > 0.0);
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
}

/// Completes cursor, look, and movement updates before systems sample the
/// camera's final transform for the current frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlyCameraUpdateSet;

impl Default for FlyCamera {
    fn default() -> Self {
        Self { speed: 24.0 }
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
    configured_perspective: PerspectiveMode,
    feel: CameraFeelSettings,
}

/// Camera feel toggles and scales mirrored from retained settings, already sanitized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFeelSettings {
    pub fov_effects_scale: f32,
    pub distortion_scale: f32,
    pub view_bobbing: bool,
    pub cinematic_camera: bool,
    pub camera_shake: bool,
    pub damage_bob: f32,
    pub mouse_sensitivity: f32,
    pub gamepad_look_sensitivity: f32,
    pub touch_look_sensitivity: f32,
}

impl CameraFeelSettings {
    fn from_settings(settings: &UserSettings) -> Self {
        let unit = |value: f32| {
            if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        Self {
            fov_effects_scale: unit(settings.video.fov_effects_scale),
            distortion_scale: unit(settings.video.distortion_scale),
            view_bobbing: settings.video.view_bobbing,
            cinematic_camera: settings.video.cinematic_camera,
            camera_shake: settings.video.camera_shake,
            damage_bob: unit(settings.video.damage_bob),
            mouse_sensitivity: settings.controls.mouse_sensitivity,
            gamepad_look_sensitivity: settings.controls.gamepad_look_sensitivity,
            touch_look_sensitivity: settings.controls.touch_look_sensitivity,
        }
    }

    /// The router's linear look multiplier for the controlling device.
    #[must_use]
    pub fn look_multiplier(&self, mode: semantic_input::InputMode) -> f32 {
        match mode {
            semantic_input::InputMode::KeyboardMouse => self.mouse_sensitivity,
            semantic_input::InputMode::GamePad => self.gamepad_look_sensitivity,
            semantic_input::InputMode::Touch => self.touch_look_sensitivity,
        }
    }
}

impl Default for CameraSettingsAuthority {
    fn default() -> Self {
        let settings = UserSettings::default();
        Self {
            generation: 0,
            horizontal_fov_degrees: settings.video.horizontal_fov_degrees,
            perspective: settings.gameplay.default_perspective,
            configured_perspective: settings.gameplay.default_perspective,
            feel: CameraFeelSettings::from_settings(&settings),
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
        if self.configured_perspective != settings.gameplay.default_perspective {
            self.configured_perspective = settings.gameplay.default_perspective;
            self.perspective = self.configured_perspective;
        }
        self.feel = CameraFeelSettings::from_settings(settings);
        Ok(())
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// The Bedrock FOV setting is vertical for a full-window viewport. The
    /// legacy accessor name is retained with the settings storage field.
    #[must_use]
    pub const fn horizontal_fov_degrees(&self) -> f32 {
        self.horizontal_fov_degrees
    }

    #[must_use]
    pub const fn perspective(&self) -> PerspectiveMode {
        self.perspective
    }

    #[must_use]
    pub const fn feel(&self) -> &CameraFeelSettings {
        &self.feel
    }

    /// Advances the configured first-person and third-person camera cycle.
    pub fn cycle_perspective(&mut self) {
        self.perspective = next_perspective(self.perspective);
    }

    pub fn reset_perspective(&mut self) {
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
            // Vanilla reverse orbit retains the full player look vector;
            // looking at the player keeps global Y as up.
            let translation = subject_translation + forward * THIRD_PERSON_RADIUS_BLOCKS;
            Transform::from_translation(translation).looking_at(subject_translation, Vec3::Y)
        }
    }
}

/// Resolves the third-person camera boom against collision data.
///
/// The camera is a radius-0.2 axis-aligned point sweep. The query is
/// camera-lenient: unknown runtime ids and unloaded cells are skipped, so the
/// boom stops at known solid geometry rather than collapsing onto the subject
/// near custom blocks or chunk edges. A genuinely malformed query leaves the
/// full preset boom rather than gluing the camera to the model.
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
    let LenientCollisionBoxes { value, skipped } = world
        .collision_boxes_camera_lenient(camera.swept(sweep))
        .unwrap_or_default();
    let fraction = value
        .into_iter()
        .filter_map(|collision| segment_entry_fraction(origin, sweep, collision.grown(radius)))
        .fold(1.0_f64, f64::min);
    if fraction < 1.0 {
        let hit_distance = f64::from(delta.length()) * fraction;
        let safe_distance =
            (hit_distance - f64::from(THIRD_PERSON_COLLISION_EPSILON_BLOCKS)).max(0.0);
        pose.translation = subject_translation + delta.normalize_or_zero() * safe_distance as f32;
    }
    record_boom_telemetry(pose.translation.distance(subject_translation), skipped);
    pose
}

/// Bounded boom telemetry: logs only when the resolved radius bucket or skip
/// tally changes, so a steady third-person view logs once, not every frame.
fn record_boom_telemetry(radius: f32, skipped: LenientSkipCounts) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static LAST_STATE: AtomicU32 = AtomicU32::new(u32::MAX);

    let bucket = (radius.clamp(0.0, THIRD_PERSON_RADIUS_BLOCKS) * 4.0).round() as u32;
    let unknown = skipped.unknown_runtime_id.min(0xFF);
    let unloaded = skipped.unloaded_chunk.min(0xFF);
    let state = bucket | (unknown << 16) | (unloaded << 24);
    if LAST_STATE.swap(state, Ordering::Relaxed) == state {
        return;
    }
    debug!(
        boom_radius = radius,
        skipped_unknown_runtime_id = skipped.unknown_runtime_id,
        skipped_unloaded_chunk = skipped.unloaded_chunk,
        "third-person camera boom resolved"
    );
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
    presentation_paused: bool,
    path_anchor: Option<Vec3>,
    last_path_position: Option<Vec3>,
    look_target: Option<Vec3>,
    elapsed_seconds: f32,
}

impl AutoFly {
    #[must_use]
    pub const fn new(enabled: bool) -> Self {
        Self::with_startup_capture(enabled, enabled)
    }

    #[must_use]
    pub const fn with_startup_capture(enabled: bool, capture_pending: bool) -> Self {
        Self {
            enabled,
            capture_pending,
            presentation_paused: false,
            path_anchor: None,
            last_path_position: None,
            look_target: None,
            elapsed_seconds: 0.0,
        }
    }

    #[must_use]
    const fn presentation_paused(&self) -> bool {
        self.presentation_paused
    }
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    #[must_use]
    pub const fn controls_acceptance_camera(&self) -> bool {
        self.enabled || self.presentation_paused
    }

    pub fn set_look_target(&mut self, target: Vec3) {
        self.look_target = Some(target);
    }

    pub fn pause_for_stable_presentation(&mut self) {
        if self.enabled {
            self.enabled = false;
            self.presentation_paused = true;
        }
    }

    pub fn resume_after_stable_presentation(&mut self) {
        if self.presentation_paused {
            self.enabled = true;
            self.presentation_paused = false;
        }
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

/// Converts Bedrock's full-viewport FOV setting to Bevy's vertical radians.
///
/// Native `getNormalizedViewportSize` measures viewport fractions of the full
/// screen, not its pixel aspect. A full-window camera therefore keeps the
/// configured vertical angle; the projection matrix applies width/height.
#[must_use]
pub fn projection_fov_radians(fov_degrees: f32) -> f32 {
    let degrees = if fov_degrees.is_finite() {
        fov_degrees
    } else {
        UserSettings::default().video.horizontal_fov_degrees
    };
    degrees.to_radians().clamp(MIN_FOV_RADIANS, MAX_FOV_RADIANS)
}

/// Returns a finite positive projection aspect, including minimized windows.
fn window_aspect(window: &Window) -> f32 {
    let aspect = window.resolution.width() / window.resolution.height();
    if aspect.is_finite() && aspect > 0.0 {
        aspect
    } else {
        DEFAULT_ASPECT_RATIO
    }
}

pub fn spawn_fly_camera(
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
            fov: projection_fov_radians(settings.horizontal_fov_degrees()),
            aspect_ratio: window_aspect(&window),
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

pub fn update_camera_fov(
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<CameraSettingsAuthority>,
    time: Res<Time>,
    inputs: Res<CameraFovInputs>,
    mut fov_state: ResMut<CameraFovState>,
    server: Res<ServerCameraView>,
    mut cameras: Query<&mut Projection, With<FlyCamera>>,
) {
    let modifier = fov_state.advance(inputs.target_modifier(), time.delta_secs());
    let base = settings.horizontal_fov_degrees();
    let fov_degrees = server.fov_override_degrees(base).unwrap_or(base * modifier);
    let vertical = projection_fov_radians(fov_degrees);
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = projection.as_mut() {
            perspective.fov = vertical;
            perspective.aspect_ratio = window_aspect(&window);
        }
    }
}

pub fn update_perspective(
    input: crate::observations::InputObservation<'_>,
    mut settings: ResMut<CameraSettingsAuthority>,
) {
    if !input.phase(Action::CyclePerspective).pressed {
        return;
    }
    settings.cycle_perspective();
}

pub fn look_angles(yaw: f32, pitch: f32, mouse_delta: Vec2, sensitivity: Vec2) -> (f32, f32) {
    let yaw = yaw - mouse_delta.x * sensitivity.x;
    let pitch = (pitch - mouse_delta.y * sensitivity.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    (yaw, pitch)
}

pub fn input_is_active(window: &Window, cursor: &CursorOptions) -> bool {
    window.focused && cursor.grab_mode == CursorGrabMode::Locked && !cursor.visible
}

fn capture_cursor(cursor: &mut CursorOptions) {
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

/// Releases capture only when needed, avoiding Bevy's repeated OS grab notifications.
pub fn release_cursor(cursor: &mut Mut<CursorOptions>) {
    if cursor.grab_mode != CursorGrabMode::None || !cursor.visible {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
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

#[allow(clippy::too_many_arguments)]
pub fn update_cursor_capture(
    policy: crate::observations::CursorPolicy,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut auto_fly: ResMut<AutoFly>,
) {
    let (window, mut cursor) = window.into_inner();

    // Focus loss has priority over every capture request, including auto-fly.
    // The trusted consent popup needs a pointer whatever settings the scene behind it declares.
    if !window.focused || policy.consent {
        release_cursor(&mut cursor);
        clear_controller_input(&mut keys, &mut mouse_buttons, &mut mouse_motion);
        auto_fly.capture_pending = false;
        return;
    }

    let steals = policy.steals_mouse;
    if policy.absorbs_input || steals == Some(false) {
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

    let active = input_is_active(window, &cursor);
    let recapture_click = !active && mouse_buttons.just_pressed(MouseButton::Left);
    if recapture_click || (steals == Some(true) && !active) || auto_fly.capture_pending {
        capture_cursor(&mut cursor);
        if recapture_click {
            // The click that transitions from an absolute UI cursor to
            // captured gameplay input is UI authority, not an attack. Remove
            // its held state so it cannot become gameplay input on the next
            // scheduled sample; the platform must deliver a later physical
            // release and press before attack can rearm.
            mouse_buttons.release(MouseButton::Left);
            mouse_motion.delta = Vec2::ZERO;
        }
        auto_fly.capture_pending = false;
    }
}

pub fn update_look(
    spyglass: (f32, Option<Res<fov::CameraFovInputs>>),
    input: crate::observations::InputObservation<'_>,
    auto_fly: Res<AutoFly>,
    settings: Res<CameraSettingsAuthority>,
    time: Res<Time>,
    mut smoother: ResMut<look::LookSmoother>,
    mut view: ResMut<LocalViewPose>,
) {
    if auto_fly.presentation_paused() {
        return;
    }
    let mode = input
        .snapshot()
        .map_or(semantic_input::InputMode::KeyboardMouse, |snapshot| {
            snapshot.input_mode
        });
    let dt = time.delta_secs();
    let raw = Vec2::from_array(input.look_delta()) * look::analog_frame_scale(mode, dt);
    let look_delta = if settings.feel().cinematic_camera {
        smoother.filter(raw, dt)
    } else {
        smoother.reset();
        raw
    };
    if look_delta == Vec2::ZERO {
        return;
    }

    let (yaw, pitch, roll) = view.rotation().to_euler(EulerRot::YXZ);
    let (damping, facts) = spyglass;
    let look_delta = look::spyglass_turn_delta(
        look_delta,
        facts.as_ref().is_some_and(|facts| facts.spyglass_scoping),
        damping,
    );
    // LocalViewPose stores actor rotation. Vanilla inverts the front preset's
    // polar input in camera space, then reverses the rendered forward vector
    // back into actor space. Neither operation reverses actor yaw.
    let scale = look::radians_per_routed_unit(settings.feel().look_multiplier(mode));
    let (yaw, pitch) = look_angles(yaw, pitch, look_delta, Vec2::splat(scale));
    view.set_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll));
}

pub fn update_movement(
    input: crate::observations::InputObservation<'_>,
    time: Res<Time>,
    mut auto_fly: ResMut<AutoFly>,
    local_physics: Option<&dyn crate::observations::PhysicsObservation>,
    camera: Single<&FlyCamera>,
    mut view: ResMut<LocalViewPose>,
) {
    if auto_fly.presentation_paused() {
        return;
    }
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

/// Owns camera presentation state; callers retain their input and frame scheduling.
pub struct CameraPresentationPlugin {
    pub auto_fly: bool,
    pub capture_on_start: bool,
}
impl Plugin for CameraPresentationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Touches>()
            .insert_resource(AutoFly::with_startup_capture(
                self.auto_fly,
                self.capture_on_start,
            ))
            .init_resource::<CameraSettingsAuthority>()
            .init_resource::<CameraFovInputs>()
            .init_resource::<CameraFovState>()
            .init_resource::<look::LookSmoother>()
            .init_resource::<facts::ItemUseClock>()
            .init_resource::<WalkBobState>()
            .init_resource::<HandSwayState>()
            .init_resource::<CameraHurtState>()
            .init_resource::<ServerCameraView>()
            .init_resource::<PortalProgress>()
            .init_resource::<HeadMedium>()
            .init_resource::<VisionEffects>()
            .init_resource::<ScreenOverlays>()
            .init_resource::<ScreenEffectFacts>()
            .init_resource::<FirstPersonHandMotion>()
            .init_resource::<LocalViewPose>()
            .init_resource::<CameraPose>()
            .init_resource::<InteractionOriginSnapshot>()
            .init_resource::<LocalPlayerFrameCarrier>()
            .init_resource::<LocalAvatarPresentation>()
            .init_resource::<LocalAvatarVisibilityCarrier>();
    }
}
