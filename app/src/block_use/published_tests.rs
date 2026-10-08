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
    world.init_resource::<BlockUseRuntime>();
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

/// Publishes held use without another press while preserving the fixture's input authority.
fn hold_use(world: &mut World) {
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
}

/// Publishes an authoritative hotbar stack with the matched block palette for placement tests.
fn set_hotbar_stack(world: &mut World, slot: u16, block: Option<&str>) {
    let stack = block.map_or_else(protocol::NetworkItemStack::empty, |name| {
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let block_runtime_id = records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id;
        protocol::NetworkItemStack {
            network_id: i32::from(slot) + 1,
            count: 64,
            stack_network_id: i32::from(slot) + 41,
            block_runtime_id: i32::from_ne_bytes(block_runtime_id.to_ne_bytes()),
            ..protocol::NetworkItemStack::empty()
        }
    });
    world
        .resource_mut::<crate::player_runtime::PlayerRuntime>()
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slot,
            },
            stack,
            storage_item: None,
        }));
}

#[test]
fn hotbar_changes_preserve_the_held_line_intercept_and_repeat_deadline() {
    for (pending, placeable) in [(true, true), (false, true), (false, false)] {
        let (mut world, mut captured) = fixture();
        hold_use(&mut world);
        set_hotbar_stack(&mut world, 0, Some("minecraft:stone"));
        set_hotbar_stack(&mut world, 1, placeable.then_some("minecraft:dirt"));
        let selected = verified_use_selection(
            world.resource::<crate::player_runtime::PlayerRuntime>(),
            world.resource::<UiRuntime>(),
        )
        .unwrap();
        let mut runtime = world.resource_mut::<BlockUseRuntime>();
        runtime.selection_changed(&selected);
        runtime.intention.record(
            false,
            [0, 63, 1],
            LocalUse::Place,
            true,
            false,
            [0.5, 64.0, 0.5],
        );
        runtime.intention.record(
            true,
            [0, 63, 2],
            LocalUse::Place,
            true,
            false,
            [0.5, 64.0, 1.5],
        );
        runtime.record(
            ItemUseTrigger::SimulationTick,
            950,
            100,
            LocalUse::Place,
            RepeatClock {
                now_millis: 950,
                sneaking: false,
                speed: 0.0,
                survival: true,
            },
        );
        let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
        if pending {
            player
                .inventory
                .queue_local_hotbar_selection(1, Some(PlayerGameMode::Survival));
        } else {
            player.inventory.set_local_selected_slot(1);
        }
        world.run_system_cached(produce_block_use).unwrap();
        assert!(
            captured.drain().is_empty(),
            "switching must not send StopItemUseOn or start a new use"
        );
        let runtime = world.resource::<BlockUseRuntime>();
        assert_eq!(runtime.last_success_destination(), Some([0, 63, 2]));
        assert_eq!(runtime.intention.first_world_hit(), Some([0.5, 64.0, 0.5]));
        assert_eq!(
            runtime.intention.target(
                None,
                [0.5, 64.0, 2.5],
                [0.5, 63.5, 4.0],
                [1.0, 0.0, 0.0],
                false
            ),
            Some(gameplay::block_use::PlacementTarget {
                position: [0, 63, 2],
                face: 3
            })
        );
        let clock = RepeatClock {
            now_millis: 1_150,
            sneaking: false,
            speed: 0.0,
            survival: true,
        };
        assert_eq!(
            runtime.due(true, 101, clock),
            None,
            "switching must not reset the strict repeat deadline"
        );
        assert_eq!(
            runtime.due(
                true,
                101,
                RepeatClock {
                    now_millis: 1_151,
                    ..clock
                }
            ),
            Some((ItemUseTrigger::SimulationTick, 1_150))
        );
    }
}

#[test]
fn denied_nonblock_repeat_keeps_the_last_admitted_placement_schedule() {
    let (mut world, mut captured) = fixture();
    hold_use(&mut world);
    world
        .resource_mut::<crate::player_runtime::PlayerRuntime>()
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slot: 0,
            },
            stack: protocol::NetworkItemStack {
                network_id: 42,
                stack_network_id: 41,
                count: 1,
                ..protocol::NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    world.resource_mut::<BlockUseRuntime>().record(
        ItemUseTrigger::SimulationTick,
        500,
        100,
        LocalUse::Place,
        RepeatClock {
            now_millis: 500,
            sneaking: false,
            speed: 0.0,
            survival: true,
        },
    );
    world.run_system_cached(produce_block_use).unwrap();
    assert!(
        captured.drain().is_empty(),
        "the locally denied repeat must not emit a chest interaction"
    );
    assert_eq!(
        world.resource::<BlockUseRuntime>().due(
            true,
            102,
            RepeatClock {
                now_millis: 1_001,
                sneaking: false,
                speed: 0.0,
                survival: true
            },
        ),
        Some((ItemUseTrigger::SimulationTick, 700)),
        "an unsent interaction must not change the last admitted placement's schedule"
    );
}

#[test]
fn pending_hotbar_selection_retains_a_deferred_block_press() {
    let (mut world, mut captured) = fixture();
    world
        .resource_mut::<crate::player_runtime::PlayerRuntime>()
        .inventory
        .queue_local_hotbar_selection(1, Some(PlayerGameMode::Survival));
    world.run_system_cached(produce_block_use).unwrap();
    assert!(captured.drain().is_empty());
    assert_eq!(
        world.resource::<BlockUseRuntime>().due(
            true,
            101,
            RepeatClock {
                now_millis: 1_000,
                sneaking: false,
                speed: 0.0,
                survival: true
            },
        ),
        Some((ItemUseTrigger::PlayerInput, 1_000)),
        "selection confirmation must not discard a deferred first press"
    );
}
