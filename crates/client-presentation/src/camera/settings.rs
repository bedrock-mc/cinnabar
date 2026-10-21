//! Retained player preferences and camera-rig configuration.

use bevy::prelude::{EulerRot, Quat, Resource, Vec3};
use semantic_input::PerspectiveMode;
use ui::UserSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraSettingsError {
    StaleGeneration { previous: u64, actual: u64 },
    NonFiniteFov,
    FovOutOfRange,
}

/// Monotonic settings snapshots prevent stale or malformed UI updates from changing the camera.
#[derive(Resource, Debug, Clone, Copy)]
pub struct CameraSettingsAuthority {
    generation: u64,
    horizontal_fov_degrees: f32,
    anti_aliasing_samples: u32,
    motion_blur: ui::MotionBlurQuality,
    smaa_mode: ui::SmaaMode,
    perspective: PerspectiveMode,
    configured_perspective: PerspectiveMode,
    pub(super) freelook: bool,
    detached: Option<super::DetachedView>,
    death_rotation: Option<Quat>,
    feel: CameraFeelSettings,
    game_sensitivity: super::look::GameSensitivity,
    rig: Option<CameraRig>,
    preserve_teleport_rotation: bool,
    view_scale: [f32; 2],
    hurt_style: Option<render_api::HurtCameraStyle>,
}

/// A local mod's third-person boom: camera-local blocks (x right, y up, z back), roll and FOV change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraRig {
    pub offset: Vec3,
    pub roll_radians: f32,
    pub fov_delta_degrees: f32,
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
    /// Java Edition 1.7 player animations, hand motion and view bob.
    pub java_animations: bool,
    pub gamepad_look_sensitivity: f32,
    pub touch_look_sensitivity: f32,
}

impl CameraFeelSettings {
    /// Sanitizes feel settings as one retained camera snapshot.
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
            java_animations: settings.video.java_animations,
            gamepad_look_sensitivity: settings.controls.gamepad_look_sensitivity,
            touch_look_sensitivity: settings.controls.touch_look_sensitivity,
        }
    }
}

impl Default for CameraSettingsAuthority {
    fn default() -> Self {
        let settings = UserSettings::default();
        Self {
            generation: 0,
            horizontal_fov_degrees: settings.video.horizontal_fov_degrees,
            anti_aliasing_samples: settings.video.anti_aliasing_samples,
            motion_blur: settings.video.motion_blur,
            smaa_mode: settings.video.smaa_mode,
            perspective: settings.gameplay.default_perspective,
            configured_perspective: settings.gameplay.default_perspective,
            freelook: false,
            detached: None,
            death_rotation: None,
            feel: CameraFeelSettings::from_settings(&settings),
            game_sensitivity: Default::default(),
            rig: None,
            preserve_teleport_rotation: false,
            view_scale: [1.0, 1.0],
            hurt_style: None,
        }
    }
}

impl CameraSettingsAuthority {
    /// Applies detached ownership to the view through the shared camera authority.
    /// The look pass has already resolved whether a held user action is admitted.
    pub fn apply_detached_view(
        &mut self,
        view: &mut crate::local_player::LocalViewPose,
        detached: Option<super::DetachedView>,
        user_freelook: bool,
    ) {
        let previous = self.detached_view();
        self.set_detached_view(detached);
        if detached.is_some() || previous.is_some() {
            view.set_freelook(detached.is_some() || user_freelook && self.freelook);
        }
        view.set_detached_translation(matches!(detached, Some(super::DetachedView::Fly { .. })));
    }

    /// Atomically accepts newer settings after validating their FOV.
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
        if !(ui::MIN_FOV_DEGREES as f32..=ui::MAX_FOV_DEGREES as f32).contains(&fov) {
            return Err(CameraSettingsError::FovOutOfRange);
        }
        self.generation = generation;
        self.horizontal_fov_degrees = fov;
        self.anti_aliasing_samples = settings.video.anti_aliasing_samples;
        self.motion_blur = settings.video.motion_blur;
        self.smaa_mode = settings.video.smaa_mode;
        if self.configured_perspective != settings.gameplay.default_perspective {
            self.configured_perspective = settings.gameplay.default_perspective;
            self.perspective = self.configured_perspective;
        }
        self.feel = CameraFeelSettings::from_settings(settings);
        self.game_sensitivity
            .set_sensitivity(settings.controls.mouse_sensitivity);
        Ok(())
    }

    #[must_use]
    /// Identifies the latest accepted settings snapshot.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the requested sample count; the renderer clamps it to device support.
    pub const fn anti_aliasing_samples(&self) -> u32 {
        self.anti_aliasing_samples
    }

    /// Returns the optional camera-only motion blur quality.
    pub const fn motion_blur(&self) -> ui::MotionBlurQuality {
        self.motion_blur
    }

    /// Returns the spatial filter preference independently of device sample counts.
    pub const fn smaa_mode(&self) -> ui::SmaaMode {
        self.smaa_mode
    }

    /// The Bedrock FOV setting is vertical for a full-window viewport. The
    /// legacy accessor name is retained with the settings storage field.
    #[must_use]
    pub const fn horizontal_fov_degrees(&self) -> f32 {
        self.horizontal_fov_degrees
    }

    /// A rig or held freelook presents third-person-back so the local body renders and the viewmodel hides.
    /// While the death view is on, first person also presents third-person-back so the body's death
    /// shows; a chosen third-person mode is kept.
    #[must_use]
    pub const fn perspective(&self) -> PerspectiveMode {
        if self.detached.is_some()
            || self.rig.is_some()
            || self.freelook
            || (self.death_rotation.is_some()
                && matches!(self.perspective, PerspectiveMode::FirstPerson))
        {
            PerspectiveMode::ThirdPersonBack
        } else {
            self.perspective
        }
    }

    /// Whether the local player's death currently overrides first person.
    #[must_use]
    pub const fn death_view(&self) -> bool {
        self.death_rotation.is_some()
    }

    /// The fixed camera heading while the local player is dead: level, facing the way the body
    /// faced when it died. Look input does not turn it.
    #[must_use]
    pub const fn death_rotation(&self) -> Option<Quat> {
        self.death_rotation
    }

    /// Turns the death view on while the local player is dead, snapping the camera to a level
    /// heading from `body_rotation`'s yaw. The stored perspective is untouched, so clearing it
    /// restores the player's own mode.
    pub fn set_death_view(&mut self, body_rotation: Option<Quat>) {
        self.death_rotation = body_rotation.map(|rotation| {
            let (yaw, _, _) = rotation.to_euler(EulerRot::YXZ);
            Quat::from_rotation_y(yaw)
        });
    }

    #[must_use]
    /// Returns the optional local camera rig override.
    pub fn rig(&self) -> Option<CameraRig> {
        match self.detached {
            Some(super::DetachedView::Orbit { distance }) => Some(CameraRig {
                offset: Vec3::new(0.0, 0.0, distance),
                roll_radians: 0.0,
                fov_delta_degrees: 0.0,
            }),
            _ => self.rig,
        }
    }

    /// Selects an already validated detached view; omission releases local view ownership.
    pub fn set_detached_view(&mut self, view: Option<super::DetachedView>) {
        self.detached = view;
    }

    /// The current extension-selected view, without changing the player's saved perspective.
    pub const fn detached_view(&self) -> Option<super::DetachedView> {
        self.detached
    }

    /// A granted extension's current-frame policy for actual teleport yaw/pitch only.
    pub const fn preserves_teleport_rotation(&self) -> bool {
        self.preserve_teleport_rotation
    }

    /// Position reconciliation and teleport acknowledgment remain server-owned.
    pub fn set_preserve_teleport_rotation(&mut self, enabled: bool) {
        self.preserve_teleport_rotation = enabled;
    }

    /// Applies ephemeral view multipliers without changing saved FOV, sensitivity or perspective.
    /// Invalid native values restore the neutral view.
    pub fn set_view_scale(&mut self, fov_scale: f32, look_scale: f32) {
        self.view_scale = if fov_scale.is_finite()
            && look_scale.is_finite()
            && fov_scale > 0.0
            && fov_scale <= 1.0
            && look_scale > 0.0
            && look_scale <= 1.0
        {
            [fov_scale, look_scale]
        } else {
            [1.0, 1.0]
        };
    }

    /// Current-frame multiplier applied after ordinary FOV effects.
    pub const fn fov_scale(&self) -> f32 {
        self.view_scale[0]
    }

    /// Current-frame multiplier applied to calibrated look deltas for every input mode.
    pub const fn look_scale(&self) -> f32 {
        self.view_scale[1]
    }

    /// Applies a validated local damage effect without changing saved preferences or aim.
    pub fn set_hurt_camera_style(&mut self, style: Option<render_api::HurtCameraStyle>) {
        self.hurt_style = style;
    }

    /// Current damage styling, falling back to the latest sanitized player preference.
    pub fn hurt_camera_style(&self) -> render_api::HurtCameraStyle {
        self.hurt_style.unwrap_or_else(|| {
            render_api::HurtCameraStyle::new(self.feel.damage_bob, false)
                .expect("camera preferences sanitize damage shake")
        })
    }

    /// Non-finite rigs are dropped; `None` restores the player's own perspective.
    pub fn set_rig(&mut self, rig: Option<CameraRig>) {
        self.rig = rig.filter(|rig| {
            rig.offset.is_finite()
                && rig.roll_radians.is_finite()
                && rig.fov_delta_degrees.is_finite()
        });
    }

    #[must_use]
    /// Returns the sanitized camera feel settings.
    pub const fn feel(&self) -> &CameraFeelSettings {
        &self.feel
    }

    #[must_use]
    /// Vanilla's game sensitivity as left by every accepted sensitivity change.
    pub const fn game_sensitivity(&self) -> f32 {
        self.game_sensitivity.value()
    }

    /// Advances the configured first-person and third-person camera cycle.
    pub fn cycle_perspective(&mut self) {
        self.perspective = next_perspective(self.perspective);
    }

    /// Restores first person and releases freelook.
    pub fn reset_perspective(&mut self) {
        self.perspective = PerspectiveMode::FirstPerson;
        self.freelook = false;
        self.detached = None;
        self.death_rotation = None;
        self.preserve_teleport_rotation = false;
        self.view_scale = [1.0, 1.0];
        self.hurt_style = None;
    }
}

#[must_use]
/// Follows the vanilla first/back/front perspective cycle.
pub const fn next_perspective(current: PerspectiveMode) -> PerspectiveMode {
    match current {
        PerspectiveMode::FirstPerson => PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonBack => PerspectiveMode::ThirdPersonFront,
        PerspectiveMode::ThirdPersonFront => PerspectiveMode::FirstPerson,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Camera settings accept both ends of the Video slider's FOV range.
    #[test]
    fn camera_accepts_shared_fov_bounds() {
        for degrees in [ui::MIN_FOV_DEGREES, ui::MAX_FOV_DEGREES] {
            let mut camera = CameraSettingsAuthority::default();
            let mut settings = UserSettings::default();
            settings.video.horizontal_fov_degrees = degrees as f32;
            camera.replace(1, &settings).unwrap();
            assert_eq!(camera.horizontal_fov_degrees(), degrees as f32);
        }
    }
}
