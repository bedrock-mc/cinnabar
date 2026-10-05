//! Composition and observations for the presentation-owned camera.
use crate::app::ClientFrameSet;
use crate::local_player::{LocalViewPose, resolve_camera_pose};
use crate::semantic_controls::{
    PendingDeviceFrame, SemanticInputRuntime, SemanticInputSnapshot, SemanticRouteState,
    SemanticTouchTargets,
};
use crate::settings_runtime::RuntimeSettings;
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorOptions, PrimaryWindow, Window},
};

pub use client_presentation::camera::{
    AUTO_FLY_MAX_HORIZONTAL_BLOCKS, AUTO_FLY_PERIOD_SECONDS, AutoFly, CameraFeelSettings,
    CameraFovInputs, CameraFovState, CameraHurtState, CameraPresentationPlugin,
    CameraSettingsAuthority, CameraSettingsError, FirstPersonHandMotion, FlyCamera,
    FlyCameraUpdateSet, HandSwayState, HeadMedium, LocalHurtEvent, OverlayKind, OverlayLayer,
    PITCH_LIMIT, PortalProgress, SPYGLASS_FOV_MODIFIER, ScreenEffectFacts, ScreenEffectInputs,
    ScreenOverlays, ServerCameraSkips, ServerCameraView, THIRD_PERSON_COLLISION_EPSILON_BLOCKS,
    THIRD_PERSON_COLLISION_RADIUS_BLOCKS, THIRD_PERSON_RADIUS_BLOCKS, ViewEffect, VisionEffects,
    WalkBobState, auto_fly_offset, collision_safe_perspective_pose, compute_overlays,
    input_is_active, look_angles, look_at_target, next_perspective, perspective_pose,
    projection_fov_radians, release_cursor, spawn_fly_camera, unavailable_world_perspective_pose,
    update_camera_fov, walk_bob_effect,
};
use client_presentation::camera::{fov, look, overlay_publish};
mod facts;
mod presentation;

/// Optional developer camera input, after physical look and before movement.
#[cfg(feature = "local-mods")]
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ModCameraInputSet;
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
        app.add_plugins(CameraPresentationPlugin {
            auto_fly: self.auto_fly,
            capture_on_start: self.capture_on_start,
        })
        .init_resource::<SemanticInputRuntime>()
        .init_resource::<SemanticInputSnapshot>()
        .init_resource::<PendingDeviceFrame>()
        .init_resource::<SemanticRouteState>()
        .init_resource::<SemanticTouchTargets>()
        .init_resource::<RuntimeSettings>()
        .add_systems(
            Startup,
            (spawn_fly_camera, overlay_publish::load_overlay_textures),
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
                    apply_runtime_camera_settings,
                    presentation::collect_fov_inputs,
                    facts::collect_screen_effect_facts,
                    update_camera_fov,
                )
                    .chain()
                    .after(ClientFrameSet::SemanticFinalize)
                    .before(FlyCameraUpdateSet),
                (
                    update_cursor_capture,
                    update_perspective,
                    update_look,
                    update_movement,
                )
                    .chain()
                    .in_set(FlyCameraUpdateSet),
                (
                    facts::collect_portal_contact,
                    presentation::advance_presentation_state,
                    presentation::update_screen_overlays,
                    presentation::apply_camera_presentation,
                    overlay_publish::publish_screen_overlays,
                    facts::diagnose_portal,
                )
                    .chain()
                    .after(resolve_camera_pose)
                    .in_set(ClientFrameSet::Camera),
            ),
        );
        #[cfg(feature = "local-mods")]
        app.configure_sets(
            Update,
            ModCameraInputSet
                .in_set(FlyCameraUpdateSet)
                .after(update_look)
                .before(update_movement),
        );
    }
}

/// Forwards each retained settings revision to the camera authority once.
fn apply_runtime_camera_settings(
    runtime: Res<RuntimeSettings>,
    mut camera: ResMut<CameraSettingsAuthority>,
) {
    let (generation, settings) = runtime.user_settings_update();
    if generation > camera.generation() {
        let _ = camera.replace(generation, settings);
    }
}
/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_perspective(
    input: Res<SemanticInputSnapshot>,
    settings: ResMut<CameraSettingsAuthority>,
) {
    client_presentation::camera::update_perspective(
        client_presentation::observations::InputObservation(input.snapshot()),
        settings,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_look(
    spyglass: (
        Option<Res<crate::menu::MenuRuntime>>,
        Option<Res<fov::CameraFovInputs>>,
    ),
    input: Res<SemanticInputSnapshot>,
    auto_fly: Res<AutoFly>,
    settings: Res<CameraSettingsAuthority>,
    time: Res<Time>,
    smoother: ResMut<look::LookSmoother>,
    view: ResMut<LocalViewPose>,
) {
    client_presentation::camera::update_look(
        (
            spyglass.0.as_ref().map_or(0.0, |menu| {
                menu.spyglass_damping(
                    input
                        .snapshot()
                        .map_or(semantic_input::InputMode::KeyboardMouse, |snapshot| {
                            snapshot.input_mode
                        }),
                )
            }),
            spyglass.1,
        ),
        client_presentation::observations::InputObservation(input.snapshot()),
        auto_fly,
        settings,
        time,
        smoother,
        view,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_movement(
    input: Res<SemanticInputSnapshot>,
    time: Res<Time>,
    auto_fly: ResMut<AutoFly>,
    local_physics: Option<Res<crate::movement::LocalPhysicsController>>,
    camera: Single<&FlyCamera>,
    view: ResMut<LocalViewPose>,
) {
    client_presentation::camera::update_movement(
        client_presentation::observations::InputObservation(input.snapshot()),
        time,
        auto_fly,
        local_physics
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::PhysicsObservation),
        camera,
        view,
    );
}
/// Samples UI cursor authority immediately before presentation updates capture.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_cursor_capture(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    keys: ResMut<ButtonInput<KeyCode>>,
    mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mouse_motion: ResMut<AccumulatedMouseMotion>,
    auto_fly: ResMut<AutoFly>,
    ui: Option<Res<client_ui::ui_runtime::UiRuntime>>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    presentation: Option<Res<client_ui::ui_runtime::presentation::UiPresentationRuntime>>,
    consent: Option<Res<crate::server_experiences::input::ConsentInput>>,
) {
    let policy = client_presentation::observations::CursorPolicy {
        consent: consent.is_some_and(|consent| consent.0),
        absorbs_input: crate::screen_policy::absorbs_input(
            &player_runtime,
            ui.as_deref(),
            menu.as_deref(),
            presentation.as_deref(),
        ),
        steals_mouse: ui.as_deref().map(|ui| {
            ui.steals_mouse(
                &player_runtime,
                menu.as_deref().map(|menu| {
                    menu as &dyn client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene
                }),
            )
        }),
    };
    client_presentation::camera::update_cursor_capture(
        policy,
        window,
        keys,
        mouse_buttons,
        mouse_motion,
        auto_fly,
    );
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

#[cfg(test)]
mod tests;
