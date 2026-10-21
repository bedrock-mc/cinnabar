//! Published frame inputs preserve current attack admission rules.

use super::attack_hit_position_tests::{
    attack, attack_fixture, picked_point, unpublished_attack_fixture,
};
use super::*;
use client_presentation::aim_assist::{AimAssistFrame, ServerAimAssist};
use protocol::test_support::transactions::observe_transaction;

/// Counts actor attacks without treating a missed swing as an actor attack.
fn attacks(captured: &mut client_session::CapturedPackets) -> usize {
    captured
        .drain()
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|tx| tx.actor_use().cloned())
        .filter(|tx| tx.action == Some(protocol::ActorUseAction::Attack))
        .count()
}

/// Selects an actor through the real two-tick cache before any melee publication.
fn select_assisted_actor(world: &mut World) {
    let mut state = ServerAimAssist::default();
    state.apply(
        1,
        &protocol::CameraEvent::AimAssist(protocol::CameraAimAssistSettings {
            preset_id: "".into(),
            view_angle: [60.0; 2],
            distance: 12.0,
            target_mode: protocol::CameraAimAssistTargetMode::Distance,
            action: protocol::CameraAimAssistAction::Set,
            show_debug_render: false,
        }),
    );
    world.resource_scope(|world, mut frame: Mut<AimAssistFrame>| {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_ref().unwrap();
        let collisions = world.resource::<PhysicsCollisionRegistries>();
        let blocks = sim::PaletteWorld::new(
            stream.collision_store(),
            collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
            Default::default(),
        );
        for tick in 1..=2 {
            frame.evaluate(
                &state,
                stream.authority(),
                &blocks,
                tick,
                Vec3::new(4.5, 2.5, 8.5),
                Vec3::NEG_Z,
                None,
                None,
                |_| None,
                |_| &[],
            );
        }
        assert!(frame.target.is_some());
    });
}

/// Commits one status or lifetime event before resolving the attack press.
fn actor_event(world: &mut World, event: protocol::ActorEvent) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(sequence, protocol::WorldEvent::Actor(event))
        .unwrap();
    stream.poll([4.5, 2.620_01, 8.5], 0);
    assert_eq!(stream.committed_sequence(), sequence);
}

#[test]
fn attack_validation_shortened_reach_rejects_a_previously_published_actor() {
    let (mut world, mut captured) = attack_fixture(7.5);
    world.insert_resource(crate::interaction_override::ModInteraction {
        attack_reach: Some(0.0),
        ..Default::default()
    });
    attack(&mut world);
    assert_eq!(attacks(&mut captured), 0);
}

#[test]
fn attack_validation_assisted_capture_cannot_bind_old_geometry_to_a_replacement() {
    let (mut world, mut captured) = unpublished_attack_fixture(7.5);
    select_assisted_actor(&mut world);
    actor_use_tests::spawn(&mut world, -30.0);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    attack(&mut world);
    assert_eq!(attacks(&mut captured), 0);
}

#[test]
fn attack_validation_replacement_fallback_uses_its_fresh_intercept() {
    let (mut world, mut captured) = unpublished_attack_fixture(7.5);
    select_assisted_actor(&mut world);
    let old_point = world.resource::<AimAssistFrame>().target.unwrap().point;
    actor_use_tests::spawn(&mut world, 7.8);
    let fresh_point = picked_point(&world);
    assert_ne!(old_point.to_array(), fresh_point);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    attack(&mut world);
    let packets = captured.drain();
    let transaction = packets.iter().find_map(observe_transaction).unwrap();
    assert_eq!(transaction.actor_use().unwrap().hit_position, fresh_point);
}

#[test]
fn attack_validation_input_and_game_mode_changes_revalidate_pick_distance() {
    for assisted in [false, true] {
        for game_mode in [PlayerGameMode::Creative, PlayerGameMode::Survival] {
            let (mut world, mut captured) = unpublished_attack_fixture(2.0);
            world
                .resource_mut::<client_state::PlayerRuntime>()
                .facts
                .publish_player_game_mode(PlayerGameMode::Creative);
            {
                let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
                let stream = client.stream.as_mut().unwrap();
                stream.predict_block([4, 2, 6], 0, stream.air_block_id());
            }
            if assisted {
                select_assisted_actor(&mut world);
            }
            let mut input = world
                .resource::<client_state::SemanticInputSnapshot>()
                .snapshot()
                .unwrap()
                .clone();
            input.input_mode = semantic_input::InputMode::Touch;
            world.insert_resource(client_state::SemanticInputSnapshot(
                semantic_input::finalized::FinalizedInput::from_finalized(input),
            ));
            world
                .run_system_cached(crate::melee::prepare_melee)
                .unwrap();
            world
                .resource_mut::<client_state::PlayerRuntime>()
                .facts
                .publish_player_game_mode(game_mode);
            attack(&mut world);
            assert_eq!(attacks(&mut captured), 0, "{assisted:?} {game_mode:?}");
        }
    }
}

/// Current facts that reject an attack from the published frame.
#[derive(Clone, Copy, Debug)]
enum Invalidity {
    Reach(f32),
    Blocked,
    Dead,
    Removed,
    Replaced,
}

impl Invalidity {
    /// Changes current facts without altering the already published geometry.
    fn apply(self, world: &mut World) {
        match self {
            Self::Reach(reach) => {
                world.insert_resource(crate::interaction_override::ModInteraction {
                    attack_reach: Some(reach),
                    ..Default::default()
                });
            }
            Self::Blocked => {
                let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
                let stream = client.stream.as_mut().unwrap();
                let block = stream
                    .collision_store()
                    .sub_chunk_ref(world::SubChunkKey::new(0, 0, 0, 0))
                    .unwrap()
                    .runtime_id(0, 4, 2, 6)
                    .unwrap();
                stream.predict_block([4, 2, 8], 0, block);
            }
            Self::Dead => actor_event(
                world,
                protocol::ActorEvent::Status(protocol::ActorStatusEvent {
                    runtime_id: 99,
                    kind: protocol::ActorStatusKind::Death,
                    data: 0,
                }),
            ),
            Self::Removed => actor_event(
                world,
                protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
                    dimension: 0,
                    unique_id: 99,
                }),
            ),
            Self::Replaced => actor_use_tests::spawn(world, -30.0),
        }
    }
}

#[test]
fn attack_validation_retention_never_bypasses_fresh_target_validity() {
    for assisted in [false, true] {
        for invalidity in [
            Invalidity::Reach(0.0),
            Invalidity::Reach(0.25),
            Invalidity::Blocked,
            Invalidity::Dead,
            Invalidity::Removed,
            Invalidity::Replaced,
        ] {
            let mut outcomes = Vec::new();
            for retained in [false, true] {
                let (mut world, mut captured) = unpublished_attack_fixture(7.5);
                if assisted {
                    select_assisted_actor(&mut world);
                }
                if retained {
                    world
                        .run_system_cached(crate::melee::prepare_melee)
                        .unwrap();
                }
                invalidity.apply(&mut world);
                if !retained {
                    world
                        .run_system_cached(crate::melee::prepare_melee)
                        .unwrap();
                }
                attack(&mut world);
                outcomes.push(attacks(&mut captured));
            }
            assert_eq!(outcomes[0], 0, "fresh {assisted:?} {invalidity:?}");
            assert_eq!(outcomes[1], 0, "retained {assisted:?} {invalidity:?}");
        }
    }
}
