//! Immutable pose, ray, stack and clicked-target authority shared by interactions.
//! The existing collision ray is a bounded provisional interaction slice, not
//! complete pickable-shape or camera-origin parity.

use std::num::NonZeroU64;

use protocol::PlayerInputMode;
use sim::{PaletteWorld, Vec3};

use crate::{
    local_player::InteractionOriginSnapshot,
    mining::{FrozenMiningFrame, FrozenMiningRay, FrozenMiningSelection, FrozenMiningTarget},
    movement::PhysicsCollisionRegistries,
    runtime::world::ClientWorld,
};
use client_ui::ui_runtime::UiRuntime;

pub(crate) use gameplay::interaction_authority::{FrozenBlockObservation, within_pick_range};

/// Whether a frozen ray still belongs to the live network session.
///
/// Events committed after the freeze (actor movement, chat) do not stale it: the ray is cast
/// against the current world, whose inspected revisions the observation records. The stream's
/// actor-session id is a separate process-wide counter, not the network session generation;
/// each is checked against the matching authority captured by the ray.
pub(crate) fn ray_is_current(
    ray: &crate::local_player::FrozenInteractionOrigin,
    ui_session: u64,
    stream: &chunk_pipeline::WorldStream,
) -> bool {
    ray.session_generation() == ui_session
        && ray.actor_session_id() == stream.authority().actor_session_id()
        && ray.fifo_sequence() <= stream.committed_sequence()
}

/// The ray or world evidence behind a block observation is stale or unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BlockRayUnavailable;

pub(crate) fn observe_block(
    origin: &InteractionOriginSnapshot,
    ui: &UiRuntime,
    client_world: &ClientWorld,
    collisions: &PhysicsCollisionRegistries,
    selection: FrozenMiningSelection,
    input: (PlayerInputMode, f64, (NonZeroU64, u64), u64),
) -> Option<FrozenBlockObservation> {
    observe_block_ray(origin, ui, client_world, collisions, selection, input)
        .ok()
        .flatten()
}

/// The nearest block on the current ray; `Ok(None)` only for a verified clear ray.
pub(crate) fn observe_block_ray(
    origin: &InteractionOriginSnapshot,
    ui: &UiRuntime,
    client_world: &ClientWorld,
    collisions: &PhysicsCollisionRegistries,
    selection: FrozenMiningSelection,
    input: (PlayerInputMode, f64, (NonZeroU64, u64), u64),
) -> Result<Option<FrozenBlockObservation>, BlockRayUnavailable> {
    let (
        input_mode,
        reach,
        (input_authority_generation, input_frame_sequence),
        position_authority_generation,
    ) = input;
    let ray = origin.outbound_ray().ok_or(BlockRayUnavailable)?;
    let stream = client_world.stream.as_ref().ok_or(BlockRayUnavailable)?;
    if !ray_is_current(ray, ui.session_id(), stream) {
        return Err(BlockRayUnavailable);
    }
    let vector = |value: bevy::prelude::Vec3| {
        Vec3::new(f64::from(value.x), f64::from(value.y), f64::from(value.z))
    };
    let world = PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    let Some(hit) = world
        .block_interaction_ray_current(vector(ray.origin()), vector(ray.direction()), reach)
        .map_err(|_| BlockRayUnavailable)?
    else {
        return Ok(None);
    };
    Ok(Some(FrozenBlockObservation {
        frame: FrozenMiningFrame {
            session_generation: ray.session_generation(),
            position_authority_generation,
            input_authority_generation,
            input_frame_sequence,
            fifo_sequence: ray.fifo_sequence(),
            physics_tick: ray.physics_tick(),
            pose_generation: ray.pose_generation(),
        },
        ray: FrozenMiningRay {
            origin: ray.origin().to_array(),
            direction: ray.direction().to_array(),
            movement_world_identity: ray.world_collision_identity().clone(),
            world_identity: hit.identity.clone(),
        },
        reach,
        input_mode,
        selection,
        target: FrozenMiningTarget {
            position: hit.block_pos,
            face: hit.face,
            relative_hit: [
                hit.hit_local.x as f32,
                hit.hit_local.y as f32,
                hit.hit_local.z as f32,
            ],
            runtime_id: hit.runtime_id,
            identity: hit.identity,
        },
    }))
}

#[cfg(test)]
/// A top-face hit on `position` holding `item` in slot 2.
pub(crate) fn fixture(
    position: [i32; 3],
    face: u8,
    item: protocol::VerifiedNetworkItemStack,
) -> FrozenBlockObservation {
    let identity = sim::CollisionQuery::synthetic(()).identity;
    FrozenBlockObservation {
        frame: FrozenMiningFrame {
            session_generation: 7,
            position_authority_generation: 0,
            input_authority_generation: NonZeroU64::MIN,
            input_frame_sequence: 1,
            fifo_sequence: 1,
            physics_tick: 101,
            pose_generation: 1,
        },
        ray: FrozenMiningRay {
            origin: [0.5, 65.62, 0.5],
            direction: [0.0, -1.0, 0.0],
            movement_world_identity: identity.clone(),
            world_identity: identity.clone(),
        },
        reach: 5.7,
        input_mode: PlayerInputMode::Mouse,
        selection: FrozenMiningSelection { slot: 2, item },
        target: FrozenMiningTarget {
            position,
            face,
            relative_hit: [0.5, 1.0, 0.5],
            runtime_id: 9,
            identity,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> chunk_pipeline::WorldStream {
        chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 42,
            local_player_unique_id: 1,
            player_position: [0.0, 70.0, 0.0],
            world_spawn_position: [0, 70, 0],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        })
    }

    fn ray(
        session: u64,
        actor_session_id: u64,
        fifo_sequence: u64,
    ) -> crate::local_player::FrozenInteractionOrigin {
        let mut carrier = crate::local_player::LocalPlayerFrameCarrier::default();
        let identity = sim::CollisionQuery::synthetic(()).identity;
        carrier
            .publish(crate::local_player::LocalPlayerFrameSample {
                session_generation: session,
                actor_session_id,
                fifo_sequence,
                physics_tick: 100,
                perspective: semantic_input::PerspectiveMode::FirstPerson,
                world_collision_identity: identity,
                pose: bevy::prelude::Transform::default(),
                eye: bevy::prelude::Vec3::new(0.0, 71.62, 0.0),
                feet: bevy::prelude::Vec3::new(0.0, 70.0, 0.0),
                rotation: bevy::prelude::Quat::IDENTITY,
            })
            .unwrap();
        let mut origin = InteractionOriginSnapshot::default();
        origin.publish_from_local_player_frame(&carrier);
        origin.outbound_ray().unwrap().clone()
    }

    /// World events committed after the ray freeze (every frame on a busy server) must not
    /// stale it, nor may the stream's process-wide actor-session counter differing from the
    /// network session generation (every reconnect); another network session does.
    #[test]
    fn a_ray_survives_later_commits_and_reconnects_but_not_a_session_change() {
        let _earlier = stream();
        let mut stream = stream();
        let actor_session_id = stream.authority().actor_session_id();
        let session = actor_session_id + 5;
        let frozen = ray(session, actor_session_id, stream.committed_sequence());
        stream
            .submit(
                stream.committed_sequence() + 1,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Remove(
                    protocol::ActorRemoveEvent {
                        dimension: 0,
                        unique_id: 99,
                    },
                )),
            )
            .unwrap();
        assert!(stream.committed_sequence() > frozen.fifo_sequence());
        assert_ne!(stream.authority().actor_session_id(), session);
        assert!(ray_is_current(&frozen, session, &stream));
        assert!(!ray_is_current(&frozen, session + 1, &stream));
        assert!(!ray_is_current(&frozen, session, &self::stream()));
        assert!(!ray_is_current(
            &ray(session, actor_session_id, stream.committed_sequence() + 1),
            session,
            &stream,
        ));
    }

    #[test]
    fn connection_and_actor_sessions_are_independent() {
        let stream = stream();
        let actor_session_id = stream.authority().actor_session_id();
        let connection_session = actor_session_id + 1;
        let frozen = ray(
            connection_session,
            actor_session_id,
            stream.committed_sequence(),
        );
        assert_eq!(frozen.session_generation(), connection_session);
        assert_eq!(frozen.actor_session_id(), actor_session_id);
        assert!(ray_is_current(&frozen, connection_session, &stream));

        // Matching a stream must not make a retired connection's ray current.
        let retired = ray(
            actor_session_id,
            actor_session_id,
            stream.committed_sequence(),
        );
        assert!(!ray_is_current(&retired, connection_session, &stream));
    }

    fn at(position: [i32; 3], input_mode: PlayerInputMode, reach: f64) -> FrozenBlockObservation {
        let stack = protocol::NetworkItemStack::empty();
        let item =
            protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
        FrozenBlockObservation {
            input_mode,
            reach,
            ..fixture(position, 1, item)
        }
    }

    #[test]
    fn touch_picks_past_the_server_corner_limit_are_dropped() {
        // Eye at (0.5, 65.62, 0.5); a block down the negative axes is centre-near, corner-far.
        let far_corner = [-5, 62, -3];
        assert!(!within_pick_range(&at(
            far_corner,
            PlayerInputMode::Touch,
            6.7
        )));
        assert!(within_pick_range(&at(
            [-4, 63, -2],
            PlayerInputMode::Touch,
            6.7
        )));
        // Mouse reach cannot reach the corner limit, so only the centre rule applies.
        assert!(within_pick_range(&at(
            [-4, 63, -1],
            PlayerInputMode::Mouse,
            5.7
        )));
        assert!(!within_pick_range(&at(
            [-6, 63, 0],
            PlayerInputMode::Mouse,
            5.7
        )));
    }
}

#[cfg(test)]
mod correction_tests;
