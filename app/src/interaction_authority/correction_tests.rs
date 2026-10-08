use std::time::{Duration, Instant};

use bevy::prelude::{Quat, Transform, Vec3 as ViewVec3};

use super::*;
use crate::local_player::{LocalPlayerFrameCarrier, LocalPlayerFrameSample, LocalViewPose};
use crate::movement::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionMode,
    reconcile_candidate_physics_correction,
};

struct Floor;

impl sim::CollisionWorld for Floor {
    /// Supplies full blocks for the correction picking fixture.
    fn collision_boxes(
        &self,
        query: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        let floor = sim::Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(sim::CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

/// Runs picking from the published view against the retained world.
fn pick(
    physics: &LocalPhysicsController,
    world: &ClientWorld,
    collisions: &PhysicsCollisionRegistries,
    ui: &UiRuntime,
) -> FrozenBlockObservation {
    let stream = world.stream.as_ref().unwrap();
    let mut view = LocalViewPose::default();
    view.set_subject_position(
        ViewVec3::from_array(physics.render_eye_position().unwrap()),
        ViewVec3::from_array(physics.render_feet_position().unwrap()),
    );
    let mut carrier = LocalPlayerFrameCarrier::default();
    carrier
        .publish(LocalPlayerFrameSample {
            session_generation: ui.session_id(),
            actor_session_id: stream.authority().actor_session_id(),
            fifo_sequence: stream.committed_sequence(),
            physics_tick: physics.state().unwrap().tick,
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            world_collision_identity: physics.last_world_identity().unwrap().clone(),
            pose: Transform::from_translation(view.eye_translation()),
            eye: view.eye_translation(),
            feet: view.feet_translation(),
            rotation: Quat::IDENTITY,
        })
        .unwrap();
    let mut origin = InteractionOriginSnapshot::default();
    origin.publish_from_local_player_frame(&carrier);
    let stack = protocol::NetworkItemStack::empty();
    let item =
        protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
    observe_block_ray(
        &origin,
        ui,
        world,
        collisions,
        FrozenMiningSelection { slot: 0, item },
        (PlayerInputMode::Mouse, 5.7, (NonZeroU64::MIN, 1), 0),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn correction_picks_follow_the_visible_crosshair_while_network_position_is_corrected() {
    let breg = assets::pinned_block_registry_bytes();
    let protocol = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(breg, protocol).unwrap();
    let id = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let collisions = PhysicsCollisionRegistries::from_assets(
        breg,
        &records,
        include_bytes!("../../../crates/assets/data/block-physics-v2193.bin"),
        protocol,
    )
    .unwrap();
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        local_player_unique_id: 1,
        player_position: [4.5, 2.620_01, 8.5],
        world_spawn_position: [4, 1, 8],
        air_network_id: id("minecraft:air"),
        block_network_ids_are_hashes: false,
    });
    let mut biome_payload = vec![1, 2];
    biome_payload.extend(std::iter::repeat_n(
        0xff,
        protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count
            - 1,
    ));
    biome_payload.push(0);
    stream
        .submit(
            1,
            protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload: biome_payload,
            }),
        )
        .unwrap();
    stream
        .submit(
            2,
            protocol::WorldEvent::BlockUpdates(
                (4..=6)
                    .map(|x| protocol::BlockUpdateEvent {
                        dimension: 0,
                        position: [x, 2, 6],
                        layer: 0,
                        network_id: id("minecraft:stone"),
                    })
                    .collect(),
            ),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while stream.committed_sequence() < 2 {
        stream.poll([4.5, 2.620_01, 8.5], 0);
        assert!(
            Instant::now() < deadline,
            "fixture world decode did not complete"
        );
        std::thread::yield_now();
    }
    assert!(
        stream
            .collision_store()
            .is_sub_chunk_loaded(world::SubChunkKey::new(0, 0, 0, 0)),
        "fixture terrain must be authoritative before interaction rays"
    );
    let world = ClientWorld {
        stream: Some(stream),
        ..Default::default()
    };
    let ui = UiRuntime::new(7);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.5, 2.620_01, 8.5], 100, true);
    let frame = physics.advance(
        Duration::from_millis(125),
        sim::MovementInput::default(),
        &Floor,
    );
    let before = pick(&physics, &world, &collisions, &ui);
    assert_eq!(before.target.position, [4, 2, 6]);
    let mut corrected = frame.samples.last().unwrap().position;
    corrected[0] += 2.0;
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [4.5, 2.620_01, 8.5]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let tick = physics.state().unwrap().tick;
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        corrected,
        tick,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &Floor,
    )
    .unwrap();
    assert_eq!(physics.state().unwrap().position.x, 6.5);
    let corrected_pick = pick(&physics, &world, &collisions, &ui);
    assert_eq!(corrected_pick.ray.origin, before.ray.origin);
    assert_eq!(corrected_pick.target.position, [4, 2, 6]);

    let frame = physics.advance(
        Duration::from_millis(75),
        sim::MovementInput::default(),
        &Floor,
    );
    assert!(frame.samples.iter().all(|sample| sample.position[0] == 6.5));
    let eased_pick = pick(&physics, &world, &collisions, &ui);
    assert!(eased_pick.ray.origin[0] > 4.5 && eased_pick.ray.origin[0] < 6.5);
    assert_eq!(eased_pick.target.position, [5, 2, 6]);
    physics.advance(
        Duration::from_millis(100),
        sim::MovementInput::default(),
        &Floor,
    );
    assert_eq!(
        pick(&physics, &world, &collisions, &ui).target.position,
        [6, 2, 6]
    );
}
