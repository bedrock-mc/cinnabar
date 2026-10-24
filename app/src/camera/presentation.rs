//! Borrows current gameplay observations before camera presentation.
use bevy::prelude::{Projection, Query, Res, ResMut, Time, Transform, With};
use client_presentation::camera::presentation::{FirstPersonHandMotion, ScreenEffectFacts};
use client_presentation::camera::{
    CameraFovInputs, CameraHurtState, CameraSettingsAuthority, FlyCamera, HeadMedium,
    PortalProgress, ScreenOverlays, ServerCameraView, VisionEffects, WalkBobState,
};
use client_presentation::server_camera::ServerCameraInstructions;
use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::runtime::world::ClientWorld,
    client_interaction::movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    client_presentation::local_player::LocalViewPose,
};

/// Borrows the semantic frame, HUD effects and movement speed for this frame's FOV inputs.
pub(crate) fn collect_fov_inputs(
    input: Res<client_state::SemanticInputSnapshot>,
    settings: Res<CameraSettingsAuthority>,
    ui: Option<Res<UiRuntimeResource>>,
    physics: Option<Res<LocalPhysicsController>>,
    movement_speed: Option<Res<client_interaction::movement::LocalMovementSpeedAuthority>>,
    inputs: ResMut<CameraFovInputs>,
) {
    client_presentation::camera::presentation::collect_fov_inputs(
        client_presentation::observations::InputObservation(input.snapshot()),
        settings,
        ui.as_deref().map(|ui| &ui.0),
        physics
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::PhysicsObservation),
        movement_speed.and_then(|speed| speed.current()),
        inputs,
    );
}

/// Samples the local actor after tick preparation and clears death time when it leaves or recovers.
pub(crate) fn collect_death_fov_inputs(
    world: Option<Res<ClientWorld>>,
    partial_tick: Option<Res<client_presentation::actor_publication::ActorFramePartialTick>>,
    inputs: ResMut<CameraFovInputs>,
) {
    client_presentation::camera::actor_effects::collect_death_fov_inputs(
        world.as_deref().and_then(|world| world.stream.as_ref()),
        partial_tick,
        inputs,
    );
}

/// Composes actor damage after this frame's counters advance and before UI publication.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_actor_damage_camera_rotation(
    settings: Res<CameraSettingsAuthority>,
    hand: ResMut<FirstPersonHandMotion>,
    legacy: Option<Res<client_presentation::camera::legacy::LegacyCameraMotion>>,
    server: Res<ServerCameraView>,
    world: Option<Res<ClientWorld>>,
    partial_tick: Option<Res<client_presentation::actor_publication::ActorFramePartialTick>>,
    physics: Option<Res<LocalPhysicsController>>,
    cameras: Query<&mut Transform, With<FlyCamera>>,
) {
    client_presentation::camera::actor_effects::apply_actor_damage_camera_rotation(
        settings,
        hand,
        legacy,
        server,
        world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        partial_tick.map_or(0.0, |tick| tick.0),
        physics
            .as_deref()
            .map_or(1.0, |physics| physics.tick_alpha()),
        cameras,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_presentation_state(
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    client_world: Option<Res<ClientWorld>>,
    physics: Option<Res<LocalPhysicsController>>,
    ui: Option<Res<UiRuntimeResource>>,
    bob: ResMut<WalkBobState>,
    hurt: ResMut<CameraHurtState>,
    java: ResMut<client_presentation::camera::java::JavaCameraState>,
    legacy: ResMut<client_presentation::camera::legacy::LegacyCameraMotion>,
    hand: ResMut<FirstPersonHandMotion>,
) {
    client_presentation::camera::presentation::advance_presentation_state(
        time,
        settings,
        view,
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        physics
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::PhysicsObservation),
        ui.as_deref().map(|ui| &ui.0),
        bob,
        hurt,
        java,
        legacy,
        hand,
    );
}

/// Borrows current owner facts and forwards them at the existing system boundary.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_screen_overlays(
    player_runtime: bevy::prelude::Res<client_state::PlayerRuntime>,
    time: Res<Time>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    facts: Res<ScreenEffectFacts>,
    fov_inputs: Res<CameraFovInputs>,
    server: Res<ServerCameraView>,
    ui: Option<Res<UiRuntimeResource>>,
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
        ui.as_deref().map(|ui| &ui.0),
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
    portal: Option<Res<PortalProgress>>,
    view: Res<LocalViewPose>,
    client_world: Option<Res<ClientWorld>>,
    collisions: Option<Res<PhysicsCollisionRegistries>>,
    server: ResMut<ServerCameraView>,
    cameras: Query<(&mut Transform, Option<&mut Projection>), With<FlyCamera>>,
) {
    client_presentation::camera::presentation::apply_camera_presentation(
        time,
        settings,
        instructions,
        portal,
        view,
        client_world.as_deref().map(
            |world| client_presentation::observations::WorldObservation {
                stream: world.stream.as_ref(),
            },
        ),
        collisions
            .as_deref()
            .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
        server,
        cameras,
    );
}
