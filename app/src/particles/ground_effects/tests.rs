use super::*;
use particles::EmptyWorld;

struct Floor;
impl ParticleWorld for Floor {
    fn solid_boxes(&self, _: [f32; 3], _: [f32; 3], out: &mut Vec<[f32; 6]>) {
        out.push([-100.0, -1.0, -100.0, 100.0, 0.0, 100.0]);
    }
    fn light(&self, _: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }
    fn fluid(&self, _: [i32; 3]) -> Fluid {
        Fluid::None
    }
}

/// Routes the same descending remote motion under both render cadences.
fn descent(system: &mut ParticleSystem, hz: u32) {
    let mut track = Track::default();
    let mut draining = Vec::new();
    let mut rejected = HashSet::new();
    let wearer = GroundEffectWearer {
        runtime_id: 1,
        spawn_revision: 1,
        item: hats::fixture::ground_effect(),
    };
    for tick in 0..=hz {
        let feet = [0.0, (0.6 - tick as f32 * 1.2 / hz as f32).max(0.0), 0.0];
        // Server's landing target arrives while the body still visibly falls.
        let grounded = remote_grounded(&Floor, feet, tick as f32 / hz as f32 >= 0.15);
        contact(
            system,
            &Floor,
            wearer,
            &mut track,
            feet,
            grounded,
            true,
            1.0 / hz as f32,
            tick as u64,
            &mut draining,
            &mut rejected,
        );
    }
}

#[test]
fn a_remote_early_ground_flag_keeps_its_landing_until_visible_feet_reach_support() {
    for hz in [20, 120] {
        let mut system = ParticleSystem::default();
        descent(&mut system, hz);
        assert_eq!(system.emitter_count(), 1, "one landing at {hz} FPS");
    }
}

#[test]
fn ground_cosmetics_do_not_evict_world_emitters_when_the_global_table_is_full() {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(hats::fixture::EMITTER_EFFECT.as_bytes()));
    let ids: Vec<_> = (0..particles::MAX_EMITTERS)
        .map(|_| {
            system
                .spawn(&SpawnRequest {
                    effect: "fixture:spark".into(),
                    ..Default::default()
                })
                .unwrap()
        })
        .collect();
    descent(&mut system, 20);
    assert_eq!(system.emitter_count(), particles::MAX_EMITTERS);
    assert!(ids.into_iter().all(|id| system.contains_emitter(id)));
    system.tick(0.05, &EmptyWorld);
}

#[test]
fn the_local_driver_drops_airborne_history_on_a_same_tick_position_authority_reset() {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                runtime_id: 1,
                unique_id: 1,
                kind: protocol::ActorKind::Player {
                    uuid: [0; 16],
                    username: "Player".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    let actor = stream.authority().actor(1).unwrap();
    assert!(
        !actor.teleported,
        "a regular local feed does not carry the teleport flag"
    );
    let wearer = GroundEffectWearer {
        runtime_id: 1,
        spawn_revision: actor.spawn_revision,
        item: hats::fixture::ground_effect(),
    };
    let mut physics = LocalPhysicsController::default();
    let mut movement = client_interaction::movement::MovementTicker::default();
    movement.reset(7, 100, [0.0, 0.5 + protocol::PLAYER_NETWORK_OFFSET, 0.0]);
    physics.reanchor_network_position(
        [0.0, 0.5 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        100,
        false,
    );
    let mut system = ParticleSystem::default();
    let mut ground = GroundEffects::default();
    ground.drive(
        &mut system,
        Some((&physics, movement.interaction_authority_identity().1.get())),
        &Floor,
        &stream,
        &[wearer],
        0.05,
        1.0,
    );
    let track = ground.tracks.get_mut(&wearer).unwrap();
    for y in [1.0, 0.9, 0.7, 0.5] {
        track.cadence.advance([0.0, y, 0.0], false, false, 0.05);
    }
    // A small snapped placement retains tick 100 and need not survive as an actor flag.
    physics.reanchor_network_position([0.0, protocol::PLAYER_NETWORK_OFFSET, 0.0], 100, true);
    movement.reanchor_surface_spawn(100, [0.0, protocol::PLAYER_NETWORK_OFFSET, 0.0]);
    let epoch = movement.interaction_authority_identity().1.get();
    ground.drive(
        &mut system,
        Some((&physics, epoch)),
        &Floor,
        &stream,
        &[wearer],
        0.05,
        1.0,
    );
    // The next completed grounded observation cannot consume the pre-teleport descent.
    physics.reanchor_network_position([0.0, protocol::PLAYER_NETWORK_OFFSET, 0.0], 101, true);
    ground.drive(
        &mut system,
        Some((&physics, epoch)),
        &Floor,
        &stream,
        &[wearer],
        0.05,
        1.0,
    );
    assert_eq!(system.emitter_count(), 0);
    assert_eq!(ground.tracks[&wearer].tick, Some(101));
}
