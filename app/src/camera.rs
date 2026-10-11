//! Composition and observations for the presentation-owned camera.
use crate::local_player::resolve_camera_pose;
use bevy::prelude::*;
use client_state::ClientFrameSet;
use client_state::RuntimeSettings;
use {
    crate::semantic_controls::{
        PendingDeviceFrame, SemanticInputRuntime, SemanticRouteState, SemanticTouchTargets,
    },
    client_state::SemanticInputSnapshot,
};

use client_presentation::camera::{
    CameraPresentationPlugin, FlyCameraUpdateSet, spawn_fly_camera, update_camera_fov,
};
use client_presentation::camera::{antialiasing, motion_blur, overlay_publish};
pub(crate) mod aim_assist;
pub(crate) mod aim_highlight;
mod facts;
mod focus;
mod presentation;

/// Spawns and drives one [`Camera3d`] fly camera.
pub struct FlyCameraPlugin {
    auto_fly: bool,
    capture_on_start: bool,
}

impl FlyCameraPlugin {
    #[must_use]
    pub const fn new(auto_fly: bool) -> Self {
        Self::with_startup_capture(auto_fly, auto_fly)
    }

    #[must_use]
    pub const fn with_startup_capture(auto_fly: bool, capture_on_start: bool) -> Self {
        Self {
            auto_fly,
            capture_on_start,
        }
    }
}

impl Default for FlyCameraPlugin {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        focus::install(app);
        app.add_plugins(client_interaction::camera::CameraInputPlugin);
        app.add_plugins(CameraPresentationPlugin {
            auto_fly: self.auto_fly,
            capture_on_start: self.capture_on_start,
        })
        .init_resource::<client_presentation::aim_assist::ServerAimAssist>()
        .init_resource::<client_presentation::aim_assist::AimAssistFrame>()
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticInputSnapshot>()
        .init_resource::<client_state::ScreenFocus>()
        .init_resource::<client_state::GameplayScreenControls>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<RuntimeSettings>()
        .add_systems(
            Startup,
            (
                spawn_fly_camera,
                overlay_publish::load_overlay_textures,
                aim_highlight::load_base_textures,
            ),
        )
        .configure_sets(
            Update,
            FlyCameraUpdateSet
                .after(ClientFrameSet::SemanticFinalize)
                .before(ClientFrameSet::Physics),
        )
        .add_systems(
            Update,
            (
                (
                    antialiasing::apply_camera_antialiasing
                        .after(client_interaction::camera::apply_runtime_camera_settings)
                        .before(client_interaction::camera::collect_fov_inputs),
                    facts::collect_screen_effect_facts
                        .after(client_interaction::camera::collect_fov_inputs),
                )
                    .chain()
                    .after(ClientFrameSet::SemanticFinalize)
                    .before(FlyCameraUpdateSet),
                (
                    presentation::collect_death_fov_inputs,
                    update_camera_fov,
                    presentation::apply_actor_damage_camera_rotation
                        .in_set(client_presentation::camera::actor_effects::ActorCameraEffects),
                    overlay_publish::publish_screen_overlays,
                )
                    .chain()
                    .after(FlyCameraUpdateSet)
                    .after(ClientFrameSet::Camera)
                    .after(ClientFrameSet::ActorPreparation)
                    .before(ClientFrameSet::UiPreparation),
                update_cursor_capture
                    .before(client_interaction::camera::update_perspective)
                    .in_set(FlyCameraUpdateSet),
                (
                    facts::collect_portal_contact,
                    presentation::advance_presentation_state,
                    presentation::update_screen_overlays,
                    presentation::apply_camera_presentation,
                    facts::diagnose_portal,
                )
                    .chain()
                    .after(resolve_camera_pose)
                    .in_set(ClientFrameSet::Camera),
                motion_blur::apply_camera_motion_blur
                    .after(ClientFrameSet::Camera)
                    .before(ClientFrameSet::Interaction),
            ),
        );
    }
}

pub(crate) use focus::{mouse_input_active, update_cursor_capture};

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

#[cfg(test)]
mod tests;
