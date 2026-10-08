//! Published-tick block admission through the production producer and bounded session queue.
use super::*;
use bevy::prelude::*;
use client_ui::ui_runtime::UiRuntime;
use protocol::{ContainerIdentity, InventoryEvent, InventorySlotEvent, SlotIdentity};
use std::time::{Duration, Instant};

/// Supplies loaded synthetic terrain, a current interaction ray and one real Use press.
fn fixture() -> (World, client_session::CapturedPackets) {
    let bytes = assets::pinned_block_registry_bytes();
    let version = assets::active_content_registry_protocol();
    let records = assets::read_registry_for_protocol(bytes, version).unwrap();
    let id = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id
    };
    let collisions = PhysicsCollisionRegistries::from_assets(
        bytes,
        &records,
        include_bytes!("../../../crates/assets/data/block-physics-v2193.bin"),
        version,
    )
    .unwrap();
    let position = [4.5, 2.620_01, 8.5];
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
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
            protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
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
            protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                dimension: 0,
                position: [4, 2, 6],
                layer: 0,
                network_id: id("minecraft:chest"),
            }]),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while stream.committed_sequence() < 2 {
        stream.poll(position, 0);
        assert!(
            Instant::now() < deadline,
            "synthetic terrain decode did not finish"
        );
        std::thread::yield_now();
    }
    let mut sample = gameplay::test_support::survival_mining::completed(101);
    sample.position = position;
    let (network, captured) = NetworkHandle::stub_capturing_packets();
    let mut movement = network.movement_ticker();
    movement.reset(7, 100, position);
    movement.set_source(gameplay::movement::MovementSource::Physics);
    movement.enqueue_completed_physics(sample.clone()).unwrap();
    let mut carrier = crate::local_player::LocalPlayerFrameCarrier::default();
    let eye = Vec3::new(4.5, 2.5, 8.5);
    carrier
        .publish(crate::local_player::LocalPlayerFrameSample {
            session_generation: 7,
            actor_session_id: stream.authority().actor_session_id(),
            fifo_sequence: stream.committed_sequence(),
            physics_tick: 101,
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            world_collision_identity: sample.world_identity,
            pose: Transform::from_translation(eye),
            eye,
            feet: eye - Vec3::Y * 1.62,
            rotation: Quat::IDENTITY,
        })
        .unwrap();
    let mut origin = InteractionOriginSnapshot::default();
    origin.publish_from_local_player_frame(&carrier);
    // The frame before this one picked along the same ray.
    let mut block_use = BlockUseRuntime::default();
    block_use.retain_pick(&origin);
    let mut player = crate::player_runtime::PlayerRuntime::new(7);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let mut ui = UiRuntime::new(7);
    ui.inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slot: 0,
            },
            stack: protocol::NetworkItemStack::empty(),
            storage_item: None,
        }));
    let mut input = crate::semantic_controls::SemanticInputRuntime::default();
    let snapshot = input
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: vec![2],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.phases[Action::Use as usize].pressed);
    let mut world = World::new();
    world
        .insert_resource(crate::semantic_controls::SemanticInputSnapshot::from_finalized(snapshot));
    world.insert_resource(origin);
    world.insert_resource(input);
    world.insert_resource(ui);
    world.insert_resource(crate::menu::MenuRuntime::new(
        false,
        2,
        "block fixture".into(),
    ));
    world.insert_resource(crate::runtime::world::ClientWorld {
        stream: Some(stream),
        ..Default::default()
    });
    world.insert_resource(collisions);
    world.init_resource::<LocalMovementEffectTimeline>();
    world.init_resource::<MeleeRuntime>();
    world.insert_resource(network);
    world.insert_resource(Time::<Real>::default());
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(1_000));
    world.init_resource::<Messages<crate::audio::LocalBlockCue>>();
    world.insert_resource(player);
    world.insert_resource(block_use);
    world.init_resource::<SwingTracker>();
    world.init_resource::<crate::item_use::ItemUseRuntime>();
    world.insert_resource(movement);
    world.spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        PrimaryWindow,
    ));
    (world, captured)
}

#[test]
fn fresh_block_use_waits_for_an_unpublished_tick_before_queueing_or_allocating_requests() {
    let (mut world, mut captured) = fixture();
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    world.resource_mut::<SwingTracker>().sync_ticks(
        authority,
        101,
        &gameplay::movement::LocalMovementEffectTimeline::default(),
    );
    world.resource_mut::<SwingTracker>().published_progress(101);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured.drain().is_empty(),
        "a fresh block interaction must wait past a published unsent tick"
    );
    assert_eq!(
        world
            .resource_mut::<crate::item_use::ItemUseRuntime>()
            .next_legacy_request_id(),
        -4
    );
    assert!(
        world
            .resource::<BlockUseRuntime>()
            .due(
                true,
                101,
                RepeatClock {
                    now_millis: 1_000,
                    sneaking: false,
                    speed: 0.0,
                    survival: true,
                }
            )
            .is_some(),
        "waiting leaves the block press pending"
    );
}

#[test]
fn a_deferred_fresh_block_press_sends_on_the_next_unpublished_tick() {
    let (mut world, mut captured) = fixture();
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    world.resource_mut::<SwingTracker>().sync_ticks(
        authority,
        101,
        &gameplay::movement::LocalMovementEffectTimeline::default(),
    );
    world.resource_mut::<SwingTracker>().published_progress(101);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(captured.drain().is_empty());
    let mut sample = gameplay::test_support::survival_mining::completed(102);
    sample.position = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample)
        .unwrap();
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: vec![2],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(!snapshot.phases[Action::Use as usize].pressed);
    world
        .insert_resource(crate::semantic_controls::SemanticInputSnapshot::from_finalized(snapshot));
    world.run_system_cached(produce_block_use).unwrap();
    let packets = captured.drain();
    let ids: Vec<_> = packets
        .iter()
        .map(|packet| format!("{:?}", packet.header.id))
        .collect();
    let swing = ids.iter().position(|id| id == "AnimatePacket").unwrap();
    let transaction = ids
        .iter()
        .position(|id| id == "InventoryTransactionPacket")
        .unwrap();
    assert!(swing < transaction);
    assert!(world.resource::<BlockUseRuntime>().interacted_at(102));
}

/// A press resolves before the newest tick's movement, so it reports the previous tick's position.
#[test]
fn a_block_press_reports_the_position_before_the_newest_tick() {
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        InventoryTransactionPacketTransaction, McpePacketData,
    };
    let (mut world, mut captured) = fixture();
    let before = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut sample = gameplay::test_support::survival_mining::completed(102);
    sample.position = [before[0] + 0.25, before[1], before[2]];
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample)
        .unwrap();
    world.run_system_cached(produce_block_use).unwrap();
    let positions: Vec<_> = captured
        .drain()
        .into_iter()
        .filter_map(|packet| match packet.data {
            McpePacketData::InventoryTransactionPacket(tx) => match tx.transaction {
                InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(tx) => {
                    Some([tx.from_position.x, tx.from_position.y, tx.from_position.z])
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(positions, [before]);
}

/// Placement casts the pick of the frame before the tick, not this frame's post-tick view.
#[test]
fn a_block_press_casts_the_pick_taken_before_the_tick() {
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        InventoryTransactionPacketTransaction, McpePacketData,
    };
    let (mut world, mut captured) = fixture();
    let stream = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = stream.stream.as_ref().unwrap();
    let (actor_session_id, fifo_sequence) = (
        stream.authority().actor_session_id(),
        stream.committed_sequence(),
    );
    let eye = Vec3::new(4.5, 2.5, 8.5);
    let mut carrier = crate::local_player::LocalPlayerFrameCarrier::default();
    carrier
        .publish(crate::local_player::LocalPlayerFrameSample {
            session_generation: 7,
            actor_session_id,
            fifo_sequence,
            physics_tick: 101,
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            world_collision_identity: gameplay::test_support::survival_mining::completed(101)
                .world_identity,
            pose: Transform::from_translation(eye),
            eye,
            feet: eye - Vec3::Y * 1.62,
            rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        })
        .unwrap();
    world
        .resource_mut::<InteractionOriginSnapshot>()
        .publish_from_local_player_frame(&carrier);
    world.run_system_cached(produce_block_use).unwrap();
    let targets: Vec<_> = captured
        .drain()
        .into_iter()
        .filter_map(|packet| match packet.data {
            McpePacketData::InventoryTransactionPacket(tx) => match tx.transaction {
                InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(tx) => {
                    Some([tx.position.x, tx.position.y, tx.position.z])
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(targets, [[4, 2, 6]]);
}
