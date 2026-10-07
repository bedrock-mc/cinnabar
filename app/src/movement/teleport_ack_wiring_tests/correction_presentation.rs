use std::time::{Duration, Instant};

use bevy::prelude::{Time, Vec3};
use bevy::time::Real;

use super::*;

/// Builds a local grounded stream for correction publication tests.
fn grounded_stream(position: [f32; 3]) -> WorldStream {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let id = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: position,
        world_spawn_position: [4, 1, 8],
        air_network_id: id("minecraft:air"),
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
            WorldEvent::BlockUpdates(
                (4..=6)
                    .map(|x| protocol::BlockUpdateEvent {
                        dimension: 0,
                        position: [x, 0, 8],
                        layer: 0,
                        network_id: id("minecraft:stone"),
                    })
                    .collect(),
            ),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while stream.committed_sequence() < 2 {
        stream.poll(position, 0);
        assert!(Instant::now() < deadline, "fixture terrain did not decode");
        std::thread::yield_now();
    }
    assert!(
        stream
            .collision_store()
            .is_sub_chunk_loaded(world::SubChunkKey::new(0, 0, 0, 0))
    );
    stream
}

#[test]
fn pending_transport_correction_keeps_the_presented_view_without_advancing_authority() {
    let initial = [4.5, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 8.5];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(initial, 100, true);
    let mut ticker = authorized_ticker(false);
    ticker.reset(7, 100, initial);
    ticker.set_source(MovementSource::Physics);
    let mut app = wiring_app(ticker, physics);
    app.world_mut().resource_mut::<ClientWorld>().stream = Some(grounded_stream(initial));
    app.insert_resource(crate::camera::AutoFly::new(false))
        .init_resource::<crate::semantic_controls::SemanticInputSnapshot>()
        .add_systems(
            Update,
            super::super::advance_local_physics.after(reconcile_world_stream_before_physics),
        );
    // Two ticks and half of the next; each render frame stays within vanilla's 0.1 s clamp.
    for millis in [100, 25] {
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(millis));
        app.update();
    }
    let before = *app.world().resource::<LocalViewPose>();
    let tick = app
        .world()
        .resource::<LocalPhysicsController>()
        .state()
        .unwrap()
        .tick;
    let mut position = app
        .world()
        .resource::<LocalPhysicsController>()
        .network_position()
        .unwrap();
    position[0] += 2.0;
    let mut admitted = None;
    flush_player_auth_inputs(
        &mut app.world_mut().resource_mut::<MovementTicker>(),
        1,
        Some(evidence_context()),
        |identity, _packet| {
            admitted = Some(identity);
            Ok::<_, &'static str>(())
        },
    )
    .unwrap();
    let admitted = admitted.expect("one real transport admission");
    submit(
        &mut app,
        3,
        WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
            position,
            delta: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            subject: MovementCorrectionSubject::Player,
            on_ground: true,
            tick,
        }),
    );
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::ZERO);
    app.update();

    let physics = app.world().resource::<LocalPhysicsController>();
    let ticker = app.world().resource::<MovementTicker>();
    assert!(!ticker.can_advance_physics_frame());
    assert_eq!(ticker.pending_count(), 1);
    assert_eq!(physics.state().unwrap().tick, tick);
    assert_eq!(physics.state().unwrap().position.x, f64::from(position[0]));
    assert_eq!(
        app.world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .resolved_server_position()
            .position,
        position
    );
    assert_eq!(
        Vec3::from_array(physics.render_eye_position().unwrap()),
        before.eye_translation()
    );
    let view = app.world().resource::<LocalViewPose>();
    assert_eq!(view.eye_translation(), before.eye_translation());
    assert_eq!(view.feet_translation(), before.feet_translation());

    assert!(
        app.world_mut()
            .resource_mut::<MovementTicker>()
            .acknowledge_physics_send(admitted)
    );
    assert!(
        app.world()
            .resource::<MovementTicker>()
            .can_advance_physics_frame()
    );
    // The first tick retains the original correction as its previous sample.
    // Sample inside the following tick to observe its interpolated decay.
    let half_tick = Duration::from_secs_f64(0.5 / sim::TICKS_PER_SECOND as f64);
    for _ in 0..2 {
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(half_tick);
        app.update();
    }
    let physics = app.world().resource::<LocalPhysicsController>();
    assert_eq!(physics.state().unwrap().tick, tick + 1);
    assert!(physics.tick_alpha() > 0.0 && physics.tick_alpha() < 1.0);
    let view = app.world().resource::<LocalViewPose>();
    assert!(view.eye_translation().x > before.eye_translation().x);
    assert!(view.eye_translation().x < position[0]);
    assert_eq!(
        view.eye_translation().to_array(),
        app.world()
            .resource::<LocalPhysicsController>()
            .render_eye_position()
            .unwrap(),
        "the presentation offset is applied exactly once"
    );
}
