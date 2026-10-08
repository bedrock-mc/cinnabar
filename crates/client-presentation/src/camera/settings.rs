//! Retained player preferences and camera-rig configuration.

use bevy::prelude::{Resource, Vec3};
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
    perspective: PerspectiveMode,
    configured_perspective: PerspectiveMode,
    pub(super) freelook: bool,
    feel: CameraFeelSettings,
    game_sensitivity: super::look::GameSensitivity,
    rig: Option<CameraRig>,
    preserve_teleport_rotation: bool,
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
            perspective: settings.gameplay.default_perspective,
            configured_perspective: settings.gameplay.default_perspective,
            freelook: false,
            feel: CameraFeelSettings::from_settings(&settings),
            game_sensitivity: Default::default(),
            rig: None,
            preserve_teleport_rotation: false,
        }
    }
}

impl CameraSettingsAuthority {
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
        if !(30.0..=120.0).contains(&fov) {
            return Err(CameraSettingsError::FovOutOfRange);
        }
        self.generation = generation;
        self.horizontal_fov_degrees = fov;
        self.anti_aliasing_samples = settings.video.anti_aliasing_samples;
        self.motion_blur = settings.video.motion_blur;
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

    /// The Bedrock FOV setting is vertical for a full-window viewport. The
    /// legacy accessor name is retained with the settings storage field.
    #[must_use]
    pub const fn horizontal_fov_degrees(&self) -> f32 {
        self.horizontal_fov_degrees
    }

    /// A rig or held freelook presents third-person-back so the local body renders and the viewmodel hides.
    #[must_use]
    pub const fn perspective(&self) -> PerspectiveMode {
        if self.rig.is_some() || self.freelook {
            PerspectiveMode::ThirdPersonBack
        } else {
            self.perspective
        }
    }

    #[must_use]
    /// Returns the optional local camera rig override.
    pub const fn rig(&self) -> Option<CameraRig> {
        self.rig
    }

    /// A granted extension's current-frame policy for actual teleport yaw/pitch only.
    pub const fn preserves_teleport_rotation(&self) -> bool {
        self.preserve_teleport_rotation
    }

    /// Position reconciliation and teleport acknowledgment remain server-owned.
    pub fn set_preserve_teleport_rotation(&mut self, enabled: bool) {
        self.preserve_teleport_rotation = enabled;
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
        self.preserve_teleport_rotation = false;
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
