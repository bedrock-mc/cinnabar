//! Published-tick block admission through the production producer and bounded session queue.
use super::*;
use bevy::prelude::*;
use client_ui::ui_runtime::UiRuntime;
use protocol::{ContainerIdentity, InventoryEvent, InventorySlotEvent, SlotIdentity};
use std::time::{Duration, Instant};

#[path = "published_tests/actor_use.rs"]
mod actor_use_tests;

#[path = "published_tests/item_use.rs"]
mod item_use_tests;

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
    block_use.retain_pick(&origin, movement.interaction_authority_identity());
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
    world.insert_resource(LocalViewPose::new(eye, Quat::IDENTITY));
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

/// A press resolves before the next tick simulates, even when the last completed tick's
/// swing has already been published; its swing precedes its transaction.
#[test]
fn a_fresh_press_resolves_for_the_tick_after_the_last_completed_one() {
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
    let ids: Vec<_> = captured
        .drain()
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

/// A press reports the end position of the last completed tick, which precedes its tick.
#[test]
fn a_block_press_reports_the_last_completed_position() {
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        InventoryTransactionPacketTransaction, McpePacketData,
    };
    let (mut world, mut captured) = fixture();
    let before = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let moved = [before[0] + 0.25, before[1], before[2]];
    let mut sample = gameplay::test_support::survival_mining::completed(102);
    sample.position = moved;
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
    assert_eq!(positions, [moved]);
}

/// Publishes one frame's eye ray for the fixture session.
fn frame_origin(world: &World, eye: Vec3, rotation: Quat) -> InteractionOriginSnapshot {
    let stream = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = stream.stream.as_ref().unwrap();
    let mut carrier = crate::local_player::LocalPlayerFrameCarrier::default();
    carrier
        .publish(crate::local_player::LocalPlayerFrameSample {
            session_generation: 7,
            actor_session_id: stream.authority().actor_session_id(),
            fifo_sequence: stream.committed_sequence(),
            physics_tick: 101,
            perspective: semantic_input::PerspectiveMode::FirstPerson,
            world_collision_identity: gameplay::test_support::survival_mining::completed(101)
                .world_identity,
            pose: Transform::from_translation(eye),
            eye,
            feet: eye - Vec3::Y * 1.62,
            rotation,
        })
        .unwrap();
    let mut origin = InteractionOriginSnapshot::default();
    origin.publish_from_local_player_frame(&carrier);
    origin
}

/// Blocks targeted by the captured item-use transactions.
fn transaction_targets(captured: &mut client_session::CapturedPackets) -> Vec<[i32; 3]> {
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        InventoryTransactionPacketTransaction, McpePacketData,
    };
    captured
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
        .collect()
}

/// Placement casts the pick of the frame before the tick, not this frame's post-tick view.
#[test]
fn a_block_press_casts_the_pick_taken_before_the_tick() {
    let (mut world, mut captured) = fixture();
    let looking_up = frame_origin(
        &world,
        Vec3::new(4.5, 2.5, 8.5),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    world.insert_resource(looking_up);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

/// Reach is measured from the pre-tick eye: a chest centre 5.5 blocks from it is placed
/// against even though the moving frame's ray started 5.8 blocks away.
#[test]
fn a_moving_press_measures_reach_from_the_pre_tick_eye() {
    let (mut world, mut captured) = fixture();
    let mut sample = gameplay::test_support::survival_mining::completed(102);
    sample.position = [4.5, 2.5, 12.0];
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample)
        .unwrap();
    let ahead = frame_origin(&world, Vec3::new(4.5, 2.5, 12.3), Quat::IDENTITY);
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    world
        .resource_mut::<BlockUseRuntime>()
        .retain_pick(&ahead, authority);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

/// A correction between frames retires the pre-correction pick; the press waits for a
/// pick taken under the new authority instead of using the old eye ray.
#[test]
fn a_press_after_a_correction_does_not_reuse_the_earlier_pick() {
    let (mut world, mut captured) = fixture();
    let position = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut movement = world.resource_mut::<MovementTicker>();
    movement.reanchor_surface_spawn(101, position);
    let mut sample = gameplay::test_support::survival_mining::completed(102);
    sample.position = position;
    movement.enqueue_completed_physics(sample).unwrap();
    let looking_up = frame_origin(
        &world,
        Vec3::new(4.5, 2.5, 8.5),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
    );
    world.insert_resource(looking_up);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(transaction_targets(&mut captured).is_empty());
    assert_eq!(
        world
            .resource::<BlockUseRuntime>()
            .due(
                true,
                102,
                RepeatClock {
                    now_millis: 1_000,
                    sneaking: false,
                    speed: 0.0,
                    survival: true,
                }
            )
            .map(|(trigger, _)| trigger),
        Some(protocol::ItemUseTrigger::PlayerInput),
        "the press stays pending"
    );
}

/// While block targeting waits after a correction, a held throwable is not thrown: the
/// same press opens the chest once a valid pick exists.
#[test]
fn a_press_waiting_for_a_pick_is_not_resolved_as_item_use() {
    let (mut world, mut captured) = fixture();
    let snowball = 300;
    let mut stream = world.resource_mut::<crate::runtime::world::ClientWorld>();
    assert!(
        stream
            .stream
            .as_mut()
            .unwrap()
            .seed_item_registry(protocol::ItemRegistryEvent {
                entries: [protocol::ItemRegistryEntry {
                    identifier: "minecraft:snowball".into(),
                    network_id: snowball,
                    component_based: false,
                    version: protocol::ItemRegistryVersion::None,
                    component_digest: [0; 32],
                    negotiated_max_stack_size: Some(16),
                    canonical_empty_component_data: true,
                    item_tags: std::sync::Arc::from([]),
                }]
                .into(),
            })
    );
    world.resource_scope(|world, mut ui: Mut<UiRuntime>| {
        let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
        let extra_data: std::sync::Arc<[u8]> = std::sync::Arc::from([]);
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
                stack: protocol::NetworkItemStack {
                    network_id: snowball,
                    metadata: 0,
                    stack_network_id: 5,
                    count: 16,
                    nbt_digest: <sha2::Sha256 as sha2::Digest>::digest(&extra_data).into(),
                    block_runtime_id: 0,
                    extra_data,
                },
                storage_item: None,
            }));
    });
    world.init_resource::<client_presentation::aim_assist::AimAssistFrame>();
    world.init_resource::<crate::camera::ServerCameraView>();
    world.init_resource::<crate::local_player::LocalViewPose>();
    let position = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut movement = world.resource_mut::<MovementTicker>();
    movement.reanchor_surface_spawn(101, position);
    let mut sample = gameplay::test_support::survival_mining::completed(102);
    sample.position = position;
    movement.enqueue_completed_physics(sample).unwrap();
    // Each frame resolves build actions, publishes its pick, then resolves air use.
    for _ in 0..2 {
        world.run_system_cached(produce_block_use).unwrap();
        world
            .run_system_cached(crate::block_use::retain_block_use_pick)
            .unwrap();
        world
            .run_system_cached(crate::item_use::produce_item_use)
            .unwrap();
    }
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

/// The fixture over a stone floor, holding stone at `start` with active physics; the previous
/// frame picked along `pick` from the eye.
fn floor_fixture(start: [f32; 3], pick: Quat) -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = fixture();
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let stone = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:stone")
        .unwrap()
        .sequential_id;
    {
        let mut client_world = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client_world.stream.as_mut().unwrap();
        let floor = (3..=5)
            .flat_map(|x| (6..=9).map(move |z| [x, 0, z]))
            .map(|position| protocol::BlockUpdateEvent {
                dimension: 0,
                position,
                layer: 0,
                network_id: stone,
            })
            .collect();
        stream
            .submit(3, protocol::WorldEvent::BlockUpdates(floor))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while stream.committed_sequence() < 3 {
            stream.poll(start, 0);
            assert!(Instant::now() < deadline, "floor did not commit");
            std::thread::yield_now();
        }
    }
    world.resource_scope(|world, mut ui: Mut<UiRuntime>| {
        let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
        let extra_data: std::sync::Arc<[u8]> = std::sync::Arc::from([]);
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
                stack: protocol::NetworkItemStack {
                    network_id: 1,
                    metadata: 0,
                    stack_network_id: 5,
                    count: 64,
                    nbt_digest: <sha2::Sha256 as sha2::Digest>::digest(&extra_data).into(),
                    block_runtime_id: i32::try_from(stone).unwrap(),
                    extra_data,
                },
                storage_item: None,
            }));
    });
    let mut movement = world.resource_mut::<MovementTicker>();
    movement.reset(7, 100, start);
    let authority = movement.interaction_authority_identity();
    let mut physics = crate::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(start, 100, true);
    world.insert_resource(physics);
    let eye = Vec3::from_array(start);
    let previous = frame_origin(&world, eye, pick);
    world
        .resource_mut::<BlockUseRuntime>()
        .retain_pick(&previous, authority);
    world.insert_resource(crate::local_player::LocalViewPose::new(eye, Quat::IDENTITY));
    world.insert_resource(crate::camera::AutoFly::new(false));
    #[cfg(feature = "acceptance")]
    world.insert_resource(crate::acceptance::AcceptanceRun::new(
        None, None, false, false,
    ));
    #[cfg(not(feature = "acceptance"))]
    world.init_resource::<crate::acceptance::AcceptanceRun>();
    world.init_resource::<crate::movement::LocalMovementSpeedAuthority>();
    (world, captured)
}

/// Routes one frame of held keys and mouse buttons into the world's input snapshot.
fn hold(
    world: &mut World,
    router: &mut crate::semantic_controls::SemanticInputRuntime,
    keys: Vec<u16>,
    mouse_buttons: Vec<u8>,
) -> semantic_input::ActionSnapshot {
    let snapshot = router
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                keys,
                mouse_buttons,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(
        crate::semantic_controls::SemanticInputSnapshot::from_finalized(snapshot.clone()),
    );
    snapshot
}

/// A placement resolves before the tick that walks into its cell, so that tick's movement
/// collides with the placed block instead of reporting the player inside it.
#[test]
fn a_placement_before_the_tick_blocks_movement_into_its_cell() {
    let start = [4.5, 2.620_01, 8.32];
    // The previous frame looked down at the floor ahead; walking forward faces it.
    let (mut world, mut captured) = floor_fixture(start, Quat::from_rotation_x(-1.103));
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    let snapshot = hold(&mut world, &mut router, vec![0x1a], vec![2]);
    assert!(snapshot.movement[1] > 0.0, "the fixture walks forward");

    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 0, 7]]);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(50));
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
    let moved = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap();
    assert_eq!(moved.tick, 101);
    // The placed cell spans z 7..8; the player's half-width box stops at its face.
    assert!(moved.position[2] < start[2], "the tick walked forward");
    assert!(
        moved.position[2] - sim::PLAYER_WIDTH as f32 * 0.5 >= 8.0 - 1.0e-4,
        "movement collided with the placed block: {moved:?}"
    );
}

/// Holds Use from a still pose at one frame interval, pressing on the frame at 50 ms, and
/// returns each attempt with the tick it precedes. Only the press may land on a frame that
/// simulates no tick.
fn held_attempts(frame_micros: u64) -> Vec<(u64, [i32; 3])> {
    // A pick steep enough to land on the floor ahead, then on the block placed there.
    let (mut world, mut captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    // The reanchor discards its first elapsed time; spend it so both rates tick together.
    hold(&mut world, &mut router, Vec::new(), Vec::new());
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::ZERO);
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
    let mut attempts = Vec::new();
    let mut elapsed = 0;
    while elapsed < 800_000 {
        elapsed += frame_micros;
        let buttons = if elapsed >= 50_000 {
            vec![2]
        } else {
            Vec::new()
        };
        hold(&mut world, &mut router, Vec::new(), buttons);
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_micros(frame_micros));
        let before = world.resource::<MovementTicker>().completed_tick();
        world.run_system_cached(produce_block_use).unwrap();
        let targets = transaction_targets(&mut captured);
        world
            .run_system_cached(crate::movement::advance_local_physics)
            .unwrap();
        let simulated = world.resource::<MovementTicker>().completed_tick() > before;
        assert!(
            simulated || targets.is_empty() || attempts.is_empty(),
            "a held repeat was sent on a frame without a tick at {elapsed} us"
        );
        attempts.extend(targets.into_iter().map(|target| (before + 1, target)));
    }
    attempts
}

/// Held repeats resolve before simulation ticks, so a fast render rate attempts on the same
/// ticks and targets as one frame per tick, and never between ticks.
#[test]
fn held_repeats_follow_simulation_ticks_not_render_frames() {
    let per_tick = held_attempts(50_000);
    assert!(per_tick.len() >= 2, "the hold repeats: {per_tick:?}");
    assert_eq!(per_tick[0], (101, [4, 0, 7]));
    // 160 fps divides the 50 ms tick exactly, so ticks start at the same instants.
    assert_eq!(held_attempts(6_250), per_tick);
}

/// Drives a tapped attack plus a Use press in one frame; melee resolves the attack against
/// `crosshair` before the next frame. Returns the block transactions sent over both frames.
fn attack_tap_then_use(crosshair: crate::melee::Crosshair) -> Vec<[i32; 3]> {
    let (mut world, mut captured) = fixture();
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    let frame = |router: &mut crate::semantic_controls::SemanticInputRuntime, tap: bool| {
        router
            .route_and_finalize(semantic_input::DeviceFrame {
                keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                    mouse_buttons: vec![2],
                    mouse_edges: semantic_input::ButtonEdges {
                        pressed: if tap { vec![1, 2] } else { Vec::new() },
                        released: if tap { vec![1] } else { Vec::new() },
                    },
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap()
    };
    let tapped = frame(&mut router, true);
    let attack = tapped.phases[Action::Attack as usize];
    assert!(
        attack.pressed && !attack.held,
        "the attack was tapped between frames"
    );
    assert!(tapped.phases[Action::Use as usize].pressed);
    world.insert_resource(crate::semantic_controls::SemanticInputSnapshot::from_finalized(tapped));
    world.run_system_cached(produce_block_use).unwrap();
    // Melee runs later in the frame and resolves the same tap.
    let mut melee = world.resource_mut::<crate::melee::MeleeRuntime>();
    assert!(melee.observe_input(true, false));
    melee.resolve(
        crosshair,
        &crate::melee::PressContext {
            tick: 102,
            player_position: [4.5, 2.620_01, 8.5],
            input_mode: protocol::PlayerInputMode::Mouse,
            local_runtime_id: 42,
            selection: None,
            swing_duration: 6,
            now_millis: 1_000,
        },
        &mut gameplay::melee::SwingTracker::default(),
    );
    let held = frame(&mut router, false);
    assert!(!held.phases[Action::Use as usize].pressed);
    world.insert_resource(crate::semantic_controls::SemanticInputSnapshot::from_finalized(held));
    world.run_system_cached(produce_block_use).unwrap();
    transaction_targets(&mut captured)
}

/// An attack handled first holds off a Use pressed in the same frame: only the hit lands.
#[test]
fn a_tapped_actor_attack_holds_off_the_same_frame_use() {
    let hit = crate::melee::Crosshair::Actor(crate::melee::ActorHit {
        runtime_id: 9,
        distance: 2.0,
        point: [4.5, 2.5, 6.5],
    });
    assert!(attack_tap_then_use(hit).is_empty());
}

/// A tap that only swings at a block leaves the Use to resolve on the next frame.
#[test]
fn a_tapped_block_attack_lets_the_same_frame_use_follow() {
    assert_eq!(
        attack_tap_then_use(crate::melee::Crosshair::Block),
        [[4, 2, 6]]
    );
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
    // An active physics clock lets this frame simulate the tick the repeat precedes.
    let position = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut physics = crate::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(position, 101, true);
    world.insert_resource(physics);
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
    // The denied attempt used tick 102; the next tick keeps the admitted schedule.
    assert_eq!(
        world.resource::<BlockUseRuntime>().due(
            true,
            103,
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

/// A hotbar switch mid-hold keeps the hold: the next tick-gated repeat places the newly
/// selected block from the pre-physics build action, without a stop or a fresh start.
#[test]
fn a_hotbar_switch_mid_hold_repeats_with_the_new_block_before_the_tick() {
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        InventoryTransactionPacketTransaction, McpePacketData,
    };
    let (mut world, mut captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    hold(&mut world, &mut router, Vec::new(), vec![2]);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 0, 7]]);
    set_hotbar_stack(&mut world, 1, Some("minecraft:dirt"));
    world
        .resource_mut::<crate::player_runtime::PlayerRuntime>()
        .inventory
        .set_local_selected_slot(1);
    let mut sent = Vec::new();
    for _ in 0..8 {
        hold(&mut world, &mut router, Vec::new(), vec![2]);
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(50));
        world.run_system_cached(produce_block_use).unwrap();
        sent.extend(captured.drain());
        world
            .run_system_cached(crate::movement::advance_local_physics)
            .unwrap();
    }
    assert!(
        sent.iter()
            .all(|packet| format!("{:?}", packet.header.id) != "PlayerActionPacket"),
        "switching must not stop or restart the hold"
    );
    let repeats: Vec<_> = sent
        .into_iter()
        .filter_map(|packet| match packet.data {
            McpePacketData::InventoryTransactionPacket(tx) => match tx.transaction {
                InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(tx) => {
                    Some((tx.slot, [tx.position.x, tx.position.y, tx.position.z]))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(repeats.first(), Some(&(1, [4, 1, 7])), "{repeats:?}");
}

/// An inventory request on another slot, still awaiting its answer, does not hold back a placement.
#[test]
fn an_unrelated_inventory_request_does_not_delay_a_placement() {
    let (mut world, mut captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    world
        .resource_mut::<crate::player_runtime::PlayerRuntime>()
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
    set_hotbar_stack(&mut world, 3, Some("minecraft:dirt"));
    {
        let mut player = world.resource_mut::<crate::player_runtime::PlayerRuntime>();
        let ledger = player.inventory.ledger_mut();
        ledger.begin_world_drop(3, Some(1)).unwrap();
        assert!(ledger.pending_request_id().is_some());
    }
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    hold(&mut world, &mut router, Vec::new(), vec![2]);
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 0, 7]]);
}
