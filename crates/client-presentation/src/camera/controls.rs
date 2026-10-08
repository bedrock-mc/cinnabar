//! Camera input routing, cursor capture and deterministic acceptance movement.

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use semantic_input::Action;
use std::f32::consts::TAU;

use super::{
    AUTO_FLY_MAX_HORIZONTAL_BLOCKS, AUTO_FLY_PERIOD_SECONDS, CameraSettingsAuthority, FlyCamera,
    PITCH_LIMIT, ServerCameraView, fov, look,
};
use crate::local_player::LocalViewPose;

const AUTO_FLY_RADIUS_BLOCKS: f32 = AUTO_FLY_MAX_HORIZONTAL_BLOCKS * 0.5;
const AUTO_FLY_VERTICAL_BLOCKS: f32 = 8.0;

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
    /// Enables the deterministic acceptance path and its startup capture request.
    pub const fn new(enabled: bool) -> Self {
        Self::with_startup_capture(enabled, enabled)
    }

    #[must_use]
    /// Configures acceptance movement independently of pointer capture.
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
    /// Reports whether a capture has suspended automatic camera movement.
    const fn presentation_paused(&self) -> bool {
        self.presentation_paused
    }
    #[must_use]
    /// Reports whether the acceptance path currently advances.
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    #[must_use]
    /// Retains camera ownership while acceptance movement is paused.
    pub const fn controls_acceptance_camera(&self) -> bool {
        self.enabled || self.presentation_paused
    }

    /// Points acceptance movement toward a fixed world position.
    pub fn set_look_target(&mut self, target: Vec3) {
        self.look_target = Some(target);
    }

    /// Suspends movement while preserving acceptance-camera ownership.
    pub fn pause_for_stable_presentation(&mut self) {
        if self.enabled {
            self.enabled = false;
            self.presentation_paused = true;
        }
    }

    /// Resumes movement only after an acceptance presentation pause.
    pub fn resume_after_stable_presentation(&mut self) {
        if self.presentation_paused {
            self.enabled = true;
            self.presentation_paused = false;
        }
    }
}

#[must_use]
/// Samples the deterministic closed acceptance path.
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
/// Resolves a world look direction, retaining identity for coincident positions.
pub fn look_at_target(position: Vec3, target: Vec3) -> Quat {
    if position.distance_squared(target) <= f32::EPSILON {
        return Quat::IDENTITY;
    }
    Transform::from_translation(position)
        .looking_at(target, Vec3::Y)
        .rotation
}

/// Cycles perspective when the routed action is pressed outside freelook.
pub fn update_perspective(
    input: crate::observations::InputObservation<'_>,
    mut settings: ResMut<CameraSettingsAuthority>,
) {
    if !input.phase(Action::CyclePerspective).pressed || input.phase(Action::Freelook).held {
        return;
    }
    settings.cycle_perspective();
}

/// Applies routed look deltas with the camera pitch bounds.
pub fn look_angles(yaw: f32, pitch: f32, mouse_delta: Vec2, sensitivity: Vec2) -> (f32, f32) {
    let yaw = yaw - mouse_delta.x * sensitivity.x;
    let pitch = (pitch - mouse_delta.y * sensitivity.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    (yaw, pitch)
}

/// Requires focused and captured gameplay input.
pub fn input_is_active(window: &Window, cursor: &CursorOptions) -> bool {
    window.focused && cursor.grab_mode == CursorGrabMode::Locked && !cursor.visible
}

/// Requests the captured gameplay pointer state.
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

/// Drops held pointer and key input when gameplay capture is released.
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
/// Applies the retained input policy before accepting gameplay pointer motion.
pub fn update_cursor_capture(
    policy: crate::observations::CursorPolicy,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut auto_fly: ResMut<AutoFly>,
) {
    if policy.driven {
        return;
    }
    let (window, mut cursor) = window.into_inner();

    // Focus loss has priority over every capture request, including auto-fly.
    // The trusted consent popup needs a pointer whatever settings the scene behind it declares.
    if !window.focused || !policy.capture_allowed || policy.consent {
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
            // Recapture consumes the click; a later physical press must rearm attack.
            mouse_buttons.release(MouseButton::Left);
            mouse_motion.delta = Vec2::ZERO;
        }
        auto_fly.capture_pending = false;
    }
}

/// Routes device-scaled look input through freelook and the selected server rig.
///
/// `window_width` is the primary window's width in physical pixels, which scales mouse look.
#[allow(clippy::too_many_arguments)]
pub fn update_look(
    spyglass: (f32, Option<Res<fov::CameraFovInputs>>),
    window_width: u32,
    input: crate::observations::InputObservation<'_>,
    auto_fly: Res<AutoFly>,
    mut settings: ResMut<CameraSettingsAuthority>,
    time: Res<Time>,
    mut smoother: ResMut<look::LookSmoother>,
    mut view: ResMut<LocalViewPose>,
    mut server: Option<ResMut<ServerCameraView>>,
) {
    let held = input.phase(Action::Freelook).held && !auto_fly.presentation_paused();
    if settings.freelook != held {
        smoother.reset();
    }
    settings.freelook = held;
    view.set_freelook(held);
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

    let (yaw, pitch, roll) = view.camera_rotation().to_euler(EulerRot::YXZ);
    let feel = settings.feel();
    let degrees = match mode {
        semantic_input::InputMode::KeyboardMouse => {
            look::mouse_turn_degrees(look_delta, window_width, settings.game_sensitivity())
        }
        semantic_input::InputMode::GamePad => {
            look_delta * look::analog_degrees_per_routed_unit(feel.gamepad_look_sensitivity)
        }
        semantic_input::InputMode::Touch => {
            look_delta * look::analog_degrees_per_routed_unit(feel.touch_look_sensitivity)
        }
    };
    let (damping, facts) = spyglass;
    let degrees = look::spyglass_turn_delta(
        degrees,
        facts.as_ref().is_some_and(|facts| facts.spyglass_scoping),
        damping,
    );
    let turn = Vec2::new(degrees.x.to_radians(), degrees.y.to_radians());
    // Front-camera polar inversion and reversed forward cancel in actor space;
    // LocalViewPose retains actor yaw.
    let server_rotation = server
        .as_deref_mut()
        .and_then(|server| server.apply_look_delta(-turn));
    let (yaw, pitch) = look_angles(yaw, pitch, turn, Vec2::ONE);
    view.set_look_rotation(
        server_rotation.unwrap_or_else(|| Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll)),
    );
}

/// Advances acceptance movement or the local non-physics fly camera.
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
