use std::f32::consts::PI;

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::{
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
        touch::Touches,
    },
    prelude::*,
    window::{PrimaryWindow, Window},
};
use ui::UserSettings;

use crate::local_player::{
    CameraPose, InteractionOriginSnapshot, LocalAvatarPresentation, LocalAvatarVisibilityCarrier,
    LocalPlayerFrameCarrier, LocalViewPose,
};

pub mod antialiasing;
mod bob;
mod controls;
mod easing;
pub mod facts;
mod focus;
pub mod fov;
#[cfg(test)]
mod freelook_tests;
mod hurt;
pub mod java;
pub mod look;
pub mod motion_blur;
mod overlay;
pub mod overlay_publish;
pub mod portal_diagnostics;
mod portal_projection;
pub mod presentation;
mod rig;
mod server_view;
mod settings;
mod shake;
#[cfg(test)]
mod spawn_tests;

pub use bob::{HandSwayState, ViewEffect, WalkBobState, walk_bob_effect};
pub use controls::{
    AutoFly, auto_fly_offset, input_is_active, look_angles, look_at_target, release_cursor,
    update_cursor_capture, update_look, update_movement, update_perspective,
};
pub use focus::CursorFocus;
pub use fov::{CameraFovInputs, CameraFovState, SPYGLASS_FOV_MODIFIER};
pub use hurt::{CameraHurtState, LocalHurtEvent};
pub use overlay::{
    HeadMedium, OverlayKind, OverlayLayer, PortalProgress, ScreenEffectInputs, ScreenOverlays,
    VisionEffects, compute_overlays,
};
pub use portal_projection::{first_person_hand_fov, set_camera_projection_fov, update_camera_fov};
pub use presentation::{FirstPersonHandMotion, ScreenEffectFacts};
pub use rig::{
    collision_safe_perspective_pose, collision_safe_rig_pose, perspective_pose, rig_pose,
    unavailable_world_perspective_pose,
};
pub use server_view::{ActorView, ServerCameraSkips, ServerCameraView, ViewContext};
pub use settings::{
    CameraFeelSettings, CameraRig, CameraSettingsAuthority, CameraSettingsError, next_perspective,
};

pub const PITCH_LIMIT: f32 = 89.9_f32.to_radians();
/// Radius declared by the pinned `minecraft:camera_orbit` vanilla presets.
pub const THIRD_PERSON_RADIUS_BLOCKS: f32 = 4.0;
pub const THIRD_PERSON_COLLISION_RADIUS_BLOCKS: f32 = 0.1;
const MIN_FOV_RADIANS: f32 = PI / 180.0;
const MAX_FOV_RADIANS: f32 = PI - MIN_FOV_RADIANS;
const DEFAULT_ASPECT_RATIO: f32 = 16.0 / 9.0;

pub const AUTO_FLY_PERIOD_SECONDS: f32 = 24.0;
pub const AUTO_FLY_MAX_HORIZONTAL_BLOCKS: f32 = 128.0;

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

/// Converts the full-window FOV to vertical radians; the projection handles pixel aspect.
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

/// Spawns the camera with the selected perspective and portable anti-aliasing.
pub fn spawn_fly_camera(
    mut commands: Commands,
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    support: Res<antialiasing::CameraAntiAliasingSupport>,
) {
    let camera = FlyCamera::default();
    commands.spawn((
        Camera3d::default(),
        support.msaa(settings.anti_aliasing_samples()),
        Projection::Perspective(PerspectiveProjection {
            fov: projection_fov_radians(settings.horizontal_fov_degrees()),
            aspect_ratio: window_aspect(&window),
            near: render_api::CAMERA_NEAR_PLANE_BLOCKS,
            near_clip_plane: Vec4::new(0.0, 0.0, -1.0, -render_api::CAMERA_NEAR_PLANE_BLOCKS),
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

/// Owns camera presentation state; callers retain their input and frame scheduling.
pub struct CameraPresentationPlugin {
    pub auto_fly: bool,
    pub capture_on_start: bool,
}
impl Plugin for CameraPresentationPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(render::motion_blur::CameraMotionBlurPlugin)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<Touches>()
            .insert_resource(AutoFly::with_startup_capture(
                self.auto_fly,
                self.capture_on_start,
            ))
            .init_resource::<CameraSettingsAuthority>()
            .init_resource::<antialiasing::CameraAntiAliasingSupport>()
            .init_resource::<CameraFovInputs>()
            .init_resource::<CameraFovState>()
            .init_resource::<look::LookSmoother>()
            .init_resource::<facts::ItemUseClock>()
            .init_resource::<WalkBobState>()
            .init_resource::<HandSwayState>()
            .init_resource::<java::JavaCameraState>()
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
    fn finish(&self, app: &mut App) {
        antialiasing::install_device_support(app);
    }
}

#[cfg(test)]
#[path = "camera/rig_tests.rs"]
mod rig_tests;
