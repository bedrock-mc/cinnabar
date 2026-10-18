//! Ordered mining keeps permission and world evidence until its press is admitted.
use super::*;
use client_world::game_mode_capabilities::ability_bit;

const TARGET: [i32; 3] = [4, 2, 6];

/// Reads the fixture block through the production collision view.
fn target_block(world: &World) -> u32 {
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    sim::PaletteWorld::new(
        stream.collision_store(),
        world
            .resource::<PhysicsCollisionRegistries>()
            .registry(stream.network_id_mode()),
        stream.current_dimension(),
        Default::default(),
    )
    .primary_runtime_id(TARGET)
    .unwrap()
}

/// Denies only mining through session-bound server ability evidence.
fn deny_mining(world: &mut World) {
    let mut player = world.resource_mut::<client_state::PlayerRuntime>();
    player.facts.bind_local_abilities(7, 1, 1, true);
    player.facts.apply_local_abilities(
        7,
        1,
        1,
        protocol::AbilitiesUpdate {
            actor_unique_id: 1,
            player_permission: 0,
            command_permission: 0,
            layers: protocol::AbilityLayersEvidence::Received(
                vec![protocol::AbilityLayerEvidence {
                    layer_type: 1,
                    abilities: ability_bit::MINE,
                    values: 0,
                    fly_speed_bits: 0,
                    vertical_fly_speed_bits: 0,
                    walk_speed_bits: 0,
                }]
                .into(),
            ),
        },
    );
    assert!(!player.facts.game_mode_capabilities().unwrap().can_mine);
}

/// Counts destruction actions attached to admitted movement packets.
fn destroy_actions(packets: &[protocol::Packet]) -> usize {
    packets
        .iter()
        .flat_map(protocol::test_support::packets::input_block_actions)
        .count()
}

#[test]
fn ordered_mining_denied_clicks_keep_swings_and_actor_attacks() {
    for server_denial in [false, true] {
        let (mut app, mut captured) = app_fixture(8);
        remove_actor(app.world_mut());
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(if server_denial {
                PlayerGameMode::Creative
            } else {
                PlayerGameMode::Adventure
            });
        if server_denial {
            deny_mining(app.world_mut());
        }
        let before = target_block(app.world());
        tap(app.world_mut(), MouseButton::Left);
        frame(&mut app);
        attack_progress_tests::ticks(app.world_mut(), 102, 104);
        frame(&mut app);
        let packets = captured.drain();
        assert_eq!(
            destroy_actions(&packets),
            0,
            "denied mining must emit no block actions"
        );
        assert_eq!(target_block(app.world()), before);
        assert!(packets.iter().any(
            |p| p.kind() == protocol::swing_arm_packet(42, protocol::SwingSource::Mine).kind()
        ));
        let (mut actor_world, mut actor_packets) = mouse_fixture();
        actor_world
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(if server_denial {
                PlayerGameMode::Creative
            } else {
                PlayerGameMode::Adventure
            });
        if server_denial {
            deny_mining(&mut actor_world);
        }
        mouse_frame(
            &mut actor_world,
            MouseButton::Left,
            &[ButtonState::Pressed, ButtonState::Released],
        );
        assert_eq!(attack_count(&mut actor_packets), 1);
    }
}

#[test]
fn ordered_mining_authoritative_replacement_cancels_a_refused_press() {
    let (mut app, mut captured) = app_fixture(2);
    remove_actor(app.world_mut());
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    fill(&app, 2);
    tap(app.world_mut(), MouseButton::Left);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    let replacement = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|record| record.name.as_ref() == "minecraft:stone")
    .unwrap()
    .sequential_id;
    {
        let mut client = app
            .world_mut()
            .resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        let sequence = stream.committed_sequence() + 1;
        stream
            .submit(
                sequence,
                protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: TARGET,
                    layer: 0,
                    network_id: replacement,
                }]),
            )
            .unwrap();
        test_time::eventually_within(Duration::from_secs(5), "replacement block ingress", || {
            stream.poll([4.5, 2.620_01, 8.5], 0);
            stream.committed_sequence() == sequence
        });
    }
    assert_eq!(target_block(app.world()), replacement);
    frame(&mut app);
    assert_eq!(
        target_block(app.world()),
        replacement,
        "a stale retry must not predict over the replacement"
    );
    attack_progress_tests::ticks(app.world_mut(), 102, 104);
    frame(&mut app);
    assert_eq!(destroy_actions(&captured.drain()), 0);
    assert!(!crate::ordered_interactions::pending(app.world()));
}

#[test]
fn ordered_mining_permission_revocation_removes_a_refused_destroy_target() {
    let (mut app, mut captured) = app_fixture(2);
    remove_actor(app.world_mut());
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Creative);
    fill(&app, 2);
    tap(app.world_mut(), MouseButton::Left);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    captured.drain();
    let before = target_block(app.world());
    deny_mining(app.world_mut());
    frame(&mut app);
    assert_eq!(target_block(app.world()), before);
    attack_progress_tests::ticks(app.world_mut(), 102, 104);
    frame(&mut app);
    assert_eq!(destroy_actions(&captured.drain()), 0);
    assert!(!crate::ordered_interactions::pending(app.world()));
}
