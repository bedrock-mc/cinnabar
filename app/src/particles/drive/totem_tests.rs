use super::*;
use particles::{EmptyWorld, ParticleView, TOTEM_PARTICLE_EFFECT};

/// Supplies distinguishable authored steady and manual effects without game assets.
fn system() -> ParticleSystem {
    let mut system = ParticleSystem::default();
    for (name, rate) in [
        (
            TOTEM_PARTICLE_EFFECT,
            r#""minecraft:emitter_rate_steady":{"spawn_rate":320,"max_particles":48}"#,
        ),
        (
            "totem_manual",
            r#""minecraft:emitter_rate_manual":{"max_particles":50}"#,
        ),
    ] {
        let effect = format!(
            r#"{{"particle_effect":{{"description":{{"identifier":"minecraft:{name}","basic_render_parameters":{{"material":"particles_alpha","texture":"test"}}}},"components":{{{rate},"minecraft:emitter_lifetime_once":{{"active_time":0.15}},"minecraft:emitter_shape_point":{{}},"minecraft:particle_lifetime_expression":{{"max_lifetime":1}},"minecraft:particle_appearance_billboard":{{"size":[0.05,0.05]}}}}}}}}"#
        );
        assert!(system.register_effect(effect.as_bytes()));
    }
    system
}

/// Gives the routing boundary a player with an explicit native-position offset.
fn stream() -> WorldStream {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    for (sequence, runtime_id) in [(1, 1), (2, 42)] {
        stream
            .submit(
                sequence,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(
                    protocol::ActorSpawnEvent {
                        dimension: 0,
                        runtime_id,
                        unique_id: runtime_id as i64,
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
                    },
                )),
            )
            .unwrap();
    }
    stream
}

/// Runs an actor status through the production particle routing boundary.
fn route(system: &mut ParticleSystem, stream: &WorldStream, runtime_id: u64) {
    let registry = sim::CollisionRegistry::default();
    let world = StreamParticleWorld::new(stream, &registry, None);
    let routing = Routing {
        world: &world,
        stream,
        mode: NetworkIdMode::Sequential,
        icons: None,
    };
    route_notice(
        system,
        &routing,
        &ActorStatusNotice {
            runtime_id,
            kind: ActorStatusKind::TotemActivate,
            data: 0,
            position: [0.0; 3],
            height: None,
            eating_position: [0.0; 3],
        },
    );
}

#[test]
fn local_totem_uses_the_authored_steady_emitter_attached_to_the_player() {
    let stream = stream();
    let mut system = system();
    route(&mut system, &stream, 1);
    assert_eq!(
        system.live_particles(),
        0,
        "emission belongs to the authored rate"
    );
    system.tick(0.025, &EmptyWorld);
    assert_eq!(
        system.live_particles(),
        8,
        "activation must not invent a manual burst"
    );
    assert_eq!(system.bound_emitters().len(), 1);
    assert_eq!(system.bound_emitters()[0].1, 1);
    let draw = system.build_draw(
        &ParticleView {
            position: [0.0, 2.0, 10.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.0,
        },
        &EmptyWorld,
    );
    assert_eq!(draw.opaque.len(), 8);
    assert_eq!(
        draw.opaque[0].center_light[..3],
        [0.0, protocol::PLAYER_NETWORK_OFFSET, 0.0]
    );
    for _ in 0..5 {
        system.tick(0.025, &EmptyWorld);
    }
    assert_eq!(system.live_particles(), 48);
    system.tick(0.025, &EmptyWorld);
    assert_eq!(
        system.live_particles(),
        48,
        "the authored active window has ended"
    );
}

#[test]
fn remote_totem_status_does_not_create_a_local_activation_emitter() {
    let stream = stream();
    assert!(stream.authority().actor(42).is_some());
    let mut system = system();
    route(&mut system, &stream, 42);
    system.tick(0.025, &EmptyWorld);
    assert_eq!(system.live_particles(), 0);
    assert_eq!(system.emitter_count(), 0);
}

#[test]
fn totem_emission_follows_the_actor_without_moving_already_born_particles() {
    let mut stream = stream();
    let mut system = system();
    route(&mut system, &stream, 1);
    system.tick(0.025, &EmptyWorld);
    stream.sync_local_player_pose(&client_world::LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [0; 16],
        username: "Player".into(),
        skin: protocol::PlayerSkin::Standard(protocol::StandardSkin {
            geometry: None,
            cape: None,
            width: 64,
            height: 64,
            rgba8: vec![0; 64 * 64 * 4].into(),
        }),
        position: [2.0, 0.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        fall_fly_ticks: 0,
        yaw: 0.0,
        head_yaw: 0.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: client_world::ACTOR_SWING_TICKS,
        java_swing_ticks: client_world::ACTOR_SWING_TICKS,
        teleported: true,
        first_person: true,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    });
    assert_eq!(
        stream.authority().actor(1).unwrap().position,
        [2.0, 0.0, 0.0]
    );
    follow_bound_emitters(&mut system, &stream);
    system.tick(0.025, &EmptyWorld);
    let draw = system.build_draw(
        &ParticleView {
            position: [0.0, 2.0, 10.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.0,
        },
        &EmptyWorld,
    );
    assert_eq!(draw.opaque.len(), 16);
    for x in [0.0, 2.0] {
        assert_eq!(
            draw.opaque
                .iter()
                .filter(|particle| particle.center_light[..3]
                    == [x, protocol::PLAYER_NETWORK_OFFSET, 0.0])
                .count(),
            8
        );
    }
}
