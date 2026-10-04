//! Borrows current gameplay observations before camera presentation.
use super::{
    CameraFovInputs, CameraHurtState, CameraSettingsAuthority, FlyCamera, HandSwayState,
    HeadMedium, PortalProgress, ScreenOverlays, ServerCameraView, VisionEffects, WalkBobState,
};
use crate::{
    local_player::LocalViewPose,
    movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    runtime::world::ClientWorld,
    semantic_controls::SemanticInputSnapshot,
    server_camera::ServerCameraInstructions,
    ui_runtime::UiRuntime,
};
use bevy::prelude::{Query, Res, ResMut, Time, Transform, With};
pub use client_presentation::camera::presentation::{FirstPersonHandMotion, ScreenEffectFacts};

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_fov_inputs(
    input: Res<SemanticInputSnapshot>,
    settings: Res<CameraSettingsAuthority>,
    ui: Option<Res<UiRuntime>>,
    physics: Option<Res<LocalPhysicsController>>,
    inputs: ResMut<CameraFovInputs>,
) {
    client_presentation::camera::presentation::collect_fov_inputs(
        client_presentation::observations::InputObservation(input.snapshot()),
        settings,
        ui.as_deref(),
        physics
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::PhysicsObservation),
        inputs,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_presentation_state(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    physics: Option<Res<LocalPhysicsController>>,
    bob: ResMut<WalkBobState>,
    sway: ResMut<HandSwayState>,
    hurt: ResMut<CameraHurtState>,
    hand: ResMut<FirstPersonHandMotion>,
) {
    client_presentation::camera::presentation::advance_presentation_state(
        time,
        settings,
        view,
        physics
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::PhysicsObservation),
        bob,
        sway,
        hurt,
        hand,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_screen_overlays(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    facts: Res<ScreenEffectFacts>,
    fov_inputs: Res<CameraFovInputs>,
    server: Res<ServerCameraView>,
    ui: Option<Res<UiRuntime>>,
    client_world: Option<Res<ClientWorld>>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    portal: ResMut<PortalProgress>,
    medium: ResMut<HeadMedium>,
    vision: ResMut<VisionEffects>,
    overlays: ResMut<ScreenOverlays>,
) {
    client_presentation::camera::presentation::update_screen_overlays(
        &player_runtime,
        time,
        settings,
        view,
        facts,
        fov_inputs,
        server,
        ui.as_deref(),
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        portal,
        medium,
        vision,
        overlays,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_camera_presentation(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    instructions: Option<Res<ServerCameraInstructions>>,
    hand: Res<FirstPersonHandMotion>,
    vision: Res<VisionEffects>,
    view: Res<LocalViewPose>,
    client_world: Option<Res<ClientWorld>>,
    server: ResMut<ServerCameraView>,
    cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    client_presentation::camera::presentation::apply_camera_presentation(
        time,
        settings,
        instructions,
        hand,
        vision,
        view,
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        server,
        cameras,
    );
}
