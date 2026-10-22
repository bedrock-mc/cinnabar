//! Retained placement evidence includes every world column read before transport refusal.
use super::*;
use gameplay::movement::MovementEffectSource;

const CLICKED: [i32; 3] = [15, 2, 14];
const DESTINATION: [i32; 3] = [16, 2, 14];
const EYE: [f32; 3] = [18.5, 2.5, 16.5];
const NETWORK_POSITION: [f32; 3] = [18.5, 2.620_01, 16.5];

/// Resolves fixture blocks from the same pinned registry as the production collision owner.
fn block_id(name: &str) -> u32 {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|record| record.name.as_ref() == name)
    .unwrap()
    .sequential_id
}

/// Commits authoritative terrain through the real stream ingress before returning.
fn world_event(world: &mut World, event: protocol::WorldEvent) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream.submit(sequence, event).unwrap();
    test_time::eventually_within(Duration::from_secs(5), "boundary terrain ingress", || {
        stream.poll(NETWORK_POSITION, 0);
        stream.committed_sequence() >= sequence
    });
}

/// Writes one authoritative cell without changing inventory, input or movement authority.
fn update(world: &mut World, position: [i32; 3], block: u32) {
    world_event(
        world,
        protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
            dimension: 0,
            position,
            layer: 0,
            network_id: block,
        }]),
    );
}

/// Reads the published primary cell through the same collision view used for placement.
fn cell(world: &World, position: [i32; 3]) -> Result<u32, sim::WorldQueryError> {
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
    .primary_runtime_id(position)
}

/// Reads the destination column's terrain authority independently of mesh publication.
fn destination_revision(world: &World) -> Option<world::ChunkCollisionRevision> {
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    stream
        .collision_store()
        .collision_revision(world::ChunkKey::new(
            stream.current_dimension(),
            DESTINATION[0].div_euclid(16),
            DESTINATION[2].div_euclid(16),
        ))
}

/// Reads the held-placement inventory owner, including only already committed local uses.
fn held_count(world: &World) -> u16 {
    let player = world.resource::<client_state::PlayerRuntime>();
    let ui = world.resource::<UiRuntimeResource>();
    let selection = verified_use_selection(player, ui).unwrap();
    let revision = ui
        .inventory_ledger(player)
        .authoritative_slot_revision(selection.slot)
        .unwrap_or(0);
    world
        .resource::<BlockUseRuntime>()
        .inventory
        .predicted_selection(&selection, revision)
        .unwrap_or(selection)
        .item
        .count()
}

/// Loads a corner-crossing ray whose clicked, destination and obstruction columns differ.
fn boundary_fixture() -> (App, client_session::CapturedPackets) {
    let (mut app, mut captured) = app_fixture(8);
    let world = app.world_mut();
    remove_actor(world);
    set_hotbar_stack(world, 1, Some("minecraft:stone"));
    for (x, z) in [(1, 0), (0, 1), (1, 1)] {
        let mut payload = vec![1, 2];
        payload.extend(std::iter::repeat_n(
            0xff,
            protocol::vanilla_dimension_range(0)
                .unwrap()
                .sub_chunk_count
                - 1,
        ));
        payload.push(0);
        world_event(
            world,
            protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x,
                z,
                mode: protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                payload,
            }),
        );
    }
    update(world, CLICKED, block_id("minecraft:stone"));
    {
        let mut movement = world.resource_mut::<MovementTicker>();
        movement.reset(7, 101, NETWORK_POSITION);
        movement.set_source(gameplay::movement::MovementSource::Physics);
        let mut sample = gameplay::test_support::survival_mining::completed(102);
        sample.position = NETWORK_POSITION;
        movement.enqueue_completed_physics(sample).unwrap();
    }
    flush_movement(world);
    captured.drain();
    let direction = (Vec3::new(15.5, 2.5, 14.5) - Vec3::from_array(EYE)).normalize();
    let rotation = Quat::from_rotation_arc(Vec3::NEG_Z, direction);
    let origin = frame_origin(world, Vec3::from_array(EYE), rotation);
    let authority = world
        .resource::<MovementTicker>()
        .position_authority_identity();
    world
        .resource_mut::<BlockUseRuntime>()
        .retain_pick(&origin, authority);
    world.insert_resource(origin);
    world.insert_resource(LocalViewPose::new(Vec3::from_array(EYE), rotation));
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    assert_eq!(cell(world, DESTINATION).unwrap(), block_id("minecraft:air"));
    (app, captured)
}

/// Admits a selection prefix, then refuses the real placement batch with a full FIFO.
fn refuse_placement(app: &mut App, captured: &mut client_session::CapturedPackets) {
    fill(app, 7);
    next_slot(app.world_mut());
    tap(app.world_mut(), MouseButton::Right);
    frame(app);
    let prefix = captured.drain();
    assert_eq!(
        prefix
            .iter()
            .filter_map(equipment)
            .map(|e| e.selected_slot)
            .collect::<Vec<_>>(),
        [1]
    );
    assert!(
        prefix
            .iter()
            .filter_map(observe_transaction)
            .all(|tx| tx.item_use().is_none())
    );
    assert!(!crate::ordered_interactions::pending(app.world()));
    assert_eq!(held_count(app.world()), 64);
    assert_eq!(
        app.world()
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        0
    );
}

/// Checks cancellation has no packet, prediction, inventory or cue effects after space returns.
fn assert_cancelled(app: &mut App, captured: &mut client_session::CapturedPackets) {
    let terrain_before_retry = destination_revision(app.world());
    for _ in 0..3 {
        frame(app);
        let packets = captured.drain();
        assert!(
            packets
                .iter()
                .filter_map(observe_transaction)
                .all(|tx| tx.item_use().is_none()),
            "obsolete placement must not be admitted or retargeted"
        );
        assert!(
            packets.iter().all(|p| equipment(p).is_none()),
            "accepted selection must not replay"
        );
        assert_eq!(
            destination_revision(app.world()),
            terrain_before_retry,
            "cancelled placement must not mutate destination terrain"
        );
    }
    assert!(!crate::ordered_interactions::pending(app.world()));
    assert_eq!(held_count(app.world()), 64);
    assert_eq!(
        app.world()
            .resource::<Messages<client_audio::audio::LocalBlockCue>>()
            .len(),
        0
    );
}

/// Publishes a completed local movement sample without changing position authority.
fn complete_movement(world: &mut World, position: [f32; 3], sneaking: bool) {
    let tick = world.resource::<MovementTicker>().completed_tick() + 1;
    let mut sample = gameplay::test_support::survival_mining::completed(tick);
    sample.position = position;
    sample.sneaking = sneaking;
    sample.input.sneak_down = sneaking;
    sample.input.sneak.held = sneaking;
    sample.processed.sneaking = sneaking;
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample)
        .unwrap();
    world
        .resource_mut::<LocalMovementEffectTimeline>()
        .commit_successful_tick();
}

#[test]
fn ordered_mouse_retry_validation_local_player_entering_destination_cancels_placement() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    complete_movement(app.world_mut(), [16.5, NETWORK_POSITION[1], 14.5], false);
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(
        cell(app.world(), DESTINATION).unwrap(),
        block_id("minecraft:air")
    );
}

#[test]
fn ordered_mouse_retry_validation_standing_into_destination_cancels_placement() {
    let (mut app, mut captured) = boundary_fixture();
    let below = [16.5, protocol::PLAYER_NETWORK_OFFSET + 0.3, 14.5];
    complete_movement(app.world_mut(), below, true);
    flush_movement(app.world_mut());
    captured.drain();
    refuse_placement(&mut app, &mut captured);
    complete_movement(app.world_mut(), below, false);
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(
        cell(app.world(), DESTINATION).unwrap(),
        block_id("minecraft:air")
    );
}

#[test]
fn ordered_mouse_refused_placement_expires_even_when_world_evidence_stays_valid() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    complete_movement(app.world_mut(), [19.5, NETWORK_POSITION[1], 16.5], false);
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(
        cell(app.world(), DESTINATION).unwrap(),
        block_id("minecraft:air")
    );
}

#[test]
fn ordered_mouse_world_full_placement_cannot_overwrite_a_filled_adjacent_destination() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    let dirt = block_id("minecraft:dirt");
    update(app.world_mut(), DESTINATION, dirt);
    assert!(
        app.world()
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .unwrap()
            .prediction_generation(DESTINATION)
            .is_some(),
        "authoritative destination fill must still await mesh publication"
    );
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(cell(app.world(), DESTINATION).unwrap(), dirt);
}

#[test]
fn ordered_mouse_world_full_placement_cancels_when_the_adjacent_column_unloads() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    app.world_mut()
        .resource_mut::<crate::runtime::world::ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .evict_publication_fixture_column(world::ChunkKey::new(0, 1, 0));
    assert_cancelled(&mut app, &mut captured);
    assert!(matches!(
        cell(app.world(), DESTINATION),
        Err(sim::WorldQueryError::UnloadedChunk(_))
    ));
}

#[test]
fn ordered_mouse_world_full_placement_cancels_for_a_new_obstruction_column() {
    let (mut app, mut captured) = boundary_fixture();
    refuse_placement(&mut app, &mut captured);
    let obstruction = [17, 2, 16];
    let dirt = block_id("minecraft:dirt");
    update(app.world_mut(), obstruction, dirt);
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(cell(app.world(), obstruction).unwrap(), dirt);
    assert_eq!(
        cell(app.world(), DESTINATION).unwrap(),
        block_id("minecraft:air")
    );
}

/// Revokes one interaction ability without changing input, inventory or terrain authority.
fn deny_ability(world: &mut World, ability: u32) {
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
                    layer_type: client_world::game_mode_capabilities::ability_layer::BASE as u16,
                    abilities: ability,
                    values: 0,
                    fly_speed_bits: 0,
                    vertical_fly_speed_bits: 0,
                    walk_speed_bits: 0,
                }]
                .into(),
            ),
        },
    );
}

#[test]
fn ordered_mouse_retry_validation_build_permission_and_mode_changes_cancel_placement() {
    for mode in [
        None,
        Some(PlayerGameMode::Adventure),
        Some(PlayerGameMode::Creative),
    ] {
        let (mut app, mut captured) = boundary_fixture();
        refuse_placement(&mut app, &mut captured);
        if let Some(mode) = mode {
            app.world_mut()
                .resource_mut::<client_state::PlayerRuntime>()
                .facts
                .publish_player_game_mode(mode);
        } else {
            deny_ability(
                app.world_mut(),
                client_world::game_mode_capabilities::ability_bit::BUILD,
            );
        }
        assert_cancelled(&mut app, &mut captured);
        assert_eq!(
            cell(app.world(), DESTINATION).unwrap(),
            block_id("minecraft:air")
        );
    }
}

#[test]
fn ordered_mouse_retry_validation_switch_and_container_revocation_cancel_use() {
    use client_world::game_mode_capabilities::ability_bit;
    for (name, ability) in [
        ("minecraft:lever", ability_bit::DOORS_AND_SWITCHES),
        ("minecraft:chest", ability_bit::OPEN_CONTAINERS),
    ] {
        let (mut app, mut captured) = boundary_fixture();
        let block = block_id(name);
        update(app.world_mut(), CLICKED, block);
        refuse_placement(&mut app, &mut captured);
        deny_ability(app.world_mut(), ability);
        assert_cancelled(&mut app, &mut captured);
        assert_eq!(cell(app.world(), CLICKED).unwrap(), block);
    }
}

#[test]
fn ordered_mouse_retry_validation_actor_entering_destination_cancels_placement() {
    let (mut app, mut captured) = boundary_fixture();
    actor_use_tests::spawn(app.world_mut(), 7.5);
    refuse_placement(&mut app, &mut captured);
    world_event(
        app.world_mut(),
        protocol::WorldEvent::Actor(protocol::ActorEvent::Move(protocol::ActorMoveEvent {
            dimension: 0,
            runtime_id: 99,
            position: DESTINATION.map(|axis| Some(axis as f32 + 0.5)),
            position_origin: protocol::ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: None,
            teleported: true,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        })),
    );
    assert_cancelled(&mut app, &mut captured);
    assert_eq!(
        cell(app.world(), DESTINATION).unwrap(),
        block_id("minecraft:air")
    );
}

#[test]
fn ordered_mouse_retry_validation_new_terrain_cancels_actor_attack_and_use() {
    for (button, clear_ray) in [
        (MouseButton::Left, false),
        (MouseButton::Right, false),
        (MouseButton::Left, true),
        (MouseButton::Right, true),
    ] {
        let (mut app, mut captured) = app_fixture(8);
        if clear_ray {
            update(app.world_mut(), [4, 2, 6], block_id("minecraft:air"));
        }
        fill(&app, 8);
        tap(app.world_mut(), button);
        frame(&mut app);
        assert!(!crate::ordered_interactions::pending(app.world()));
        assert!(actor_order(&mut captured).is_empty());
        update(app.world_mut(), [4, 2, 8], block_id("minecraft:stone"));
        for _ in 0..3 {
            frame(&mut app);
            assert!(
                captured
                    .drain()
                    .iter()
                    .filter_map(observe_transaction)
                    .next()
                    .is_none(),
                "a stale actor action must cancel without retargeting or an air-use suffix"
            );
        }
        assert!(!crate::ordered_interactions::pending(app.world()));
    }
}

#[test]
fn ordered_mouse_refused_switch_and_container_use_do_not_replay() {
    for name in ["minecraft:lever", "minecraft:chest"] {
        let (mut app, mut captured) = boundary_fixture();
        update(app.world_mut(), CLICKED, block_id(name));
        refuse_placement(&mut app, &mut captured);
        let mut sent = Vec::new();
        for _ in 0..3 {
            frame(&mut app);
            sent.extend(captured.drain());
        }
        let uses: Vec<_> = sent
            .iter()
            .filter_map(observe_transaction)
            .filter_map(|tx| tx.item_use().cloned())
            .collect();
        assert!(
            uses.is_empty(),
            "refused {name} input must expire with its frame"
        );
        assert_eq!(held_count(app.world()), 64);
    }
}

#[test]
fn ordered_mouse_retry_validation_unreadable_actor_ray_keeps_initial_use_but_cannot_retry() {
    for button in [MouseButton::Left, MouseButton::Right] {
        for full in [false, true] {
            let (mut app, mut captured) = app_fixture(8);
            app.world_mut()
                .resource_mut::<crate::runtime::world::ClientWorld>()
                .stream
                .as_mut()
                .unwrap()
                .evict_publication_fixture_column(world::ChunkKey::new(0, 0, 0));
            if full {
                fill(&app, 8);
            }
            tap(app.world_mut(), button);
            frame(&mut app);
            assert_eq!(actor_order(&mut captured).len(), usize::from(!full));
            for _ in 0..3 {
                frame(&mut app);
                assert!(
                    captured
                        .drain()
                        .iter()
                        .filter_map(observe_transaction)
                        .next()
                        .is_none()
                );
            }
            assert!(!crate::ordered_interactions::pending(app.world()));
        }
    }
}

#[test]
fn ordered_mouse_retry_validation_actor_reach_change_cancels_a_creative_attack() {
    for change_mode in [true, false] {
        let (mut app, mut captured) = app_fixture(8);
        update(app.world_mut(), [4, 2, 6], block_id("minecraft:air"));
        actor_use_tests::spawn(app.world_mut(), 4.5);
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        for _ in 0..2 {
            frame(&mut app);
        }
        captured.drain();
        fill(&app, 8);
        tap(app.world_mut(), MouseButton::Left);
        frame(&mut app);
        assert!(!crate::ordered_interactions::pending(app.world()));
        assert!(actor_order(&mut captured).is_empty());
        if change_mode {
            app.world_mut()
                .resource_mut::<client_state::PlayerRuntime>()
                .facts
                .publish_player_game_mode(PlayerGameMode::Survival);
        }
        let mut sent = Vec::new();
        for _ in 0..3 {
            frame(&mut app);
            sent.extend(actor_order(&mut captured));
        }
        assert!(
            sent.is_empty(),
            "a refused tap expires even when reach stays unchanged"
        );
        assert!(
            sent.iter()
                .all(|(action, _)| *action == ActorUseAction::Attack)
        );
        assert!(!crate::ordered_interactions::pending(app.world()));
    }
}

#[test]
fn ordered_mouse_retry_validation_spectator_change_cancels_actor_use() {
    let (mut app, mut captured) = app_fixture(8);
    fill(&app, 8);
    tap(app.world_mut(), MouseButton::Right);
    frame(&mut app);
    assert!(!crate::ordered_interactions::pending(app.world()));
    assert!(actor_order(&mut captured).is_empty());
    app.world_mut()
        .resource_mut::<client_state::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Spectator);
    for _ in 0..3 {
        frame(&mut app);
        assert!(
            captured
                .drain()
                .iter()
                .filter_map(observe_transaction)
                .next()
                .is_none()
        );
    }
    assert!(!crate::ordered_interactions::pending(app.world()));
}
