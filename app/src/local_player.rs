use bevy::{
    ecs::system::SystemParam,
    prelude::{Entity, Res, ResMut, Single, Transform, With},
};

use crate::{
    camera::{CameraSettingsAuthority, FlyCamera},
    environment::WorldClock,
    movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    runtime::world::ClientWorld,
};

pub use client_presentation::local_player::{
    CameraPose, FrozenInteractionOrigin, FrozenLocalAvatarVisibility, FrozenLocalPlayerFrame,
    InteractionOriginSnapshot, LOCAL_AVATAR_EYE_HEIGHT_BLOCKS, LocalAvatarPresentation,
    LocalAvatarVisibilityCarrier, LocalPlayerFrameCarrier, LocalPlayerFrameError,
    LocalPlayerFrameReset, LocalPlayerFrameSample, LocalPlayerFrameSet, LocalViewPose,
    publish_interaction_origin, reset_local_player_session,
};

#[derive(SystemParam)]
pub(crate) struct CameraPublicationContext<'w> {
    clock: Res<'w, WorldClock>,
    physics: Res<'w, LocalPhysicsController>,
    receipt: ResMut<'w, client_presentation::local_player_camera_receipt::CameraPublicationAttempt>,
}

/// Borrows world and completed physics facts for presentation publication.
pub(crate) fn resolve_camera_pose(
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    published: ResMut<CameraPose>,
    publication: CameraPublicationContext,
    camera_transform: Single<(Entity, &mut Transform), With<FlyCamera>>,
) {
    let CameraPublicationContext {
        clock,
        physics,
        receipt,
    } = publication;
    let observed = client_presentation::local_player::CameraPublicationContext {
        clock: client_presentation::observations::SessionObservation(clock.session_generation()),
        physics: &*physics,
        receipt,
    };
    client_presentation::local_player::resolve_camera_pose(
        client_presentation::observations::WorldObservation {
            stream: client_world.stream.as_ref(),
        },
        &*collisions,
        settings,
        view,
        published,
        observed,
        camera_transform,
    );
}

/// Borrows world and completed physics facts for presentation publication.
pub(crate) fn publish_local_player_frame(
    client_world: Res<ClientWorld>,
    publication: CameraPublicationContext,
    settings: Res<CameraSettingsAuthority>,
    view: Res<LocalViewPose>,
    camera: Res<CameraPose>,
    carrier: ResMut<LocalPlayerFrameCarrier>,
) {
    let CameraPublicationContext {
        clock,
        physics,
        receipt,
    } = publication;
    let observed = client_presentation::local_player::CameraPublicationContext {
        clock: client_presentation::observations::SessionObservation(clock.session_generation()),
        physics: &*physics,
        receipt,
    };
    client_presentation::local_player::publish_local_player_frame(
        client_presentation::observations::WorldObservation {
            stream: client_world.stream.as_ref(),
        },
        observed,
        settings,
        view,
        camera,
        carrier,
    );
}
