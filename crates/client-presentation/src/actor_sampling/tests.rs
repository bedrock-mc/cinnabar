use super::*;
use protocol::{
    ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, WorldEvent,
};
use protocol_ids::ActorDataKey;
use sim::{BlockPhysicsFlags, CollisionRegistry, SurfaceResponse};
use std::sync::Arc;

/// Registry-backed lookup for material sampling, without artwork or local carriers.
struct Fluids(CollisionRegistry);

impl CollisionLookup for Fluids {
    /// Both fixture modes use the same synthetic material IDs.
    fn registry(&self, _: assets::NetworkIdMode) -> &CollisionRegistry {
        &self.0
    }

    /// Fluid sampling does not need bed directions.
    fn bed_rotation_degrees(&self, _: assets::NetworkIdMode, _: u32) -> Option<f32> {
        panic!("fluid sampling must not query beds")
    }

    /// Fluid sampling does not need block identifiers.
    fn block_identifier(&self, _: assets::NetworkIdMode, _: u32) -> Option<&str> {
        panic!("fluid sampling must not query block names")
    }
}

/// Commits a resident water cell and a crowd of small actors through the stream lanes.
fn crowd(count: u64) -> (WorldStream, Fluids) {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.5, 0.25, 0.5],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let mut payload = vec![1, 2];
    payload.extend(std::iter::repeat_n(
        0xff,
        protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count
            - 1,
    ));
    payload.push(0);
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload,
            }),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position: [0; 3],
                layer: 0,
                network_id: 1,
            }]),
        )
        .unwrap();
    for offset in 0..count {
        let runtime_id = offset + 2;
        stream
            .submit(
                offset + 3,
                WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                    dimension: 0,
                    unique_id: runtime_id as i64,
                    runtime_id,
                    kind: ActorKind::Entity {
                        identifier: "example:fluid_actor".into(),
                    },
                    position: [0.5, 0.25, 0.5],
                    velocity: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    head_yaw: 0.0,
                    body_yaw: 0.0,
                    held_item: Default::default(),
                    metadata: [ActorDataKey::Width, ActorDataKey::Height]
                        .map(|key| ActorMetadata {
                            key: key.wire_value(),
                            value: ActorMetadataValue::Float(0.4),
                        })
                        .into(),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
            )
            .unwrap();
    }
    test_time::eventually("resident fluid crowd", || {
        stream.poll([0.5, 0.25, 0.5], 0);
        stream.committed_sequence() >= count + 2
    });
    let mut registry = CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry
        .register_physics(
            1,
            [],
            0.6,
            1.0,
            1.0,
            0.875,
            BlockPhysicsFlags::WATER,
            SurfaceResponse::None,
        )
        .unwrap();
    (stream, Fluids(registry))
}

#[test]
fn repeated_actor_fluid_ticks_allocate_nothing() {
    let (mut stream, lookup) = crowd(32);
    let mut sampler = ActorFluidSampler::default();
    sampler.sample(&mut stream, &lookup);
    let (_, allocations) = test_alloc::count(|| {
        sampler.sample(&mut stream, &lookup);
    });
    for runtime_id in 2..34 {
        let actor = stream.authority().actor(runtime_id).unwrap();
        assert_eq!(actor.status.fluid, Some((true, false)));
        assert_eq!(actor.status.breathing_submerged, Some(true));
    }
    assert_eq!(allocations, 0);
}

/// Commits one test mutation after the fixture's actor spawn.
fn commit(stream: &mut WorldStream, sequence: u64, event: WorldEvent) {
    stream.submit(sequence, event).unwrap();
    test_time::eventually("fluid probe mutation", || {
        stream.poll([0.5, 0.25, 0.5], 0);
        stream.committed_sequence() >= sequence
    });
}

/// Changes one body dimension without replacing the actor's previously sampled state.
fn resize(key: ActorDataKey, value: f32) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
        dimension: 0,
        runtime_id: 2,
        metadata: Arc::from([ActorMetadata {
            key: key.wire_value(),
            value: ActorMetadataValue::Float(value),
        }]),
        properties: Arc::from([]),
        tick: 1,
    }))
}

#[test]
fn unavailable_body_retains_contact_but_updates_breathing() {
    let (mut stream, lookup) = crowd(1);
    commit(&mut stream, 4, resize(ActorDataKey::Width, 32.0));
    stream.set_actor_fluids(&[(2, false, true)]);
    stream.set_actor_breathing_liquids(&[(2, false)]);
    ActorFluidSampler::default().sample(&mut stream, &lookup);
    let status = &stream.authority().actor(2).unwrap().status;
    assert_eq!(status.fluid, Some((false, true)));
    assert_eq!(status.breathing_submerged, Some(true));
}

#[test]
fn unavailable_breathing_retains_submersion_but_updates_contact() {
    let (mut stream, lookup) = crowd(1);
    commit(&mut stream, 4, resize(ActorDataKey::Height, 2.0));
    commit(
        &mut stream,
        5,
        WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
            dimension: 0,
            position: [0, 2, 0],
            layer: 0,
            network_id: 99,
        }]),
    );
    stream.set_actor_fluids(&[(2, false, true)]);
    stream.set_actor_breathing_liquids(&[(2, true)]);
    ActorFluidSampler::default().sample(&mut stream, &lookup);
    let status = &stream.authority().actor(2).unwrap().status;
    assert_eq!(status.fluid, Some((true, false)));
    assert_eq!(status.breathing_submerged, Some(true));
}

#[test]
fn retained_buffers_do_not_replay_samples_in_a_replacement_stream() {
    let (mut first, lookup) = crowd(32);
    let mut sampler = ActorFluidSampler::default();
    sampler.sample(&mut first, &lookup);
    let (mut next, mut lookup) = crowd(1);
    lookup.0 = CollisionRegistry::new();
    lookup.0.register(0, []).unwrap();
    next.set_actor_fluids(&[(2, false, true)]);
    next.set_actor_breathing_liquids(&[(2, false)]);
    sampler.sample(&mut next, &lookup);
    let status = &next.authority().actor(2).unwrap().status;
    assert_eq!(status.fluid, Some((false, true)));
    assert_eq!(status.breathing_submerged, Some(false));
}
