use std::{num::NonZeroU64, sync::Arc};

use assets::{BlockPhysicsFlags, RegistryRecord, read_registry_for_protocol};
use bevy::{
    prelude::{App, Update, Window},
    window::PrimaryWindow,
};
use protocol::{
    BedrockSession, BlockItemInteraction, ContainerIdentity, InventoryAuthority, InventoryEvent,
    InventorySlotEvent, NetworkItemStack, PlayerInputMode, SlotIdentity, VerifiedNetworkItemStack,
};
use sha2::{Digest, Sha256};
use sim::{CollisionIdSpace, CollisionRegistryIdentity, WorldCollisionIdentity};

use super::{
    BlockUseRuntime, FrozenEmptyHandBlockUse, block_use_edge_authorized, mining_edge_authorized,
    verified_empty_hand_selection,
};
use crate::{
    mining::{
        CreativeMiningAbility, FrozenCreativeMining, FrozenMiningFrame, FrozenMiningRay,
        FrozenMiningSelection, FrozenMiningTarget,
    },
    movement::{
        MovementSource, MovementTicker, PhysicsCollisionRegistries, PhysicsMovementSample,
        PhysicsTickEvidenceContext, ProcessedMovementState, flush_player_auth_inputs,
        flush_player_auth_inputs_guarded,
    },
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

fn world_identity(revision: u64) -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            protocol: 2168,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [7; 32],
        },
        [world::ChunkCollisionRevision {
            chunk: world::ChunkKey::new(0, 0, 0),
            revision,
        }],
    )
    .unwrap()
}

fn network_item(network_id: i32) -> NetworkItemStack {
    if network_id == 0 {
        NetworkItemStack::empty()
    } else {
        let extra_data: Arc<[u8]> = Arc::from([]);
        NetworkItemStack {
            network_id,
            metadata: 0,
            stack_network_id: 41,
            count: 1,
            nbt_digest: Sha256::digest(&extra_data).into(),
            block_runtime_id: 0,
            extra_data,
        }
    }
}

fn verified_item(network_id: i32) -> VerifiedNetworkItemStack {
    let stack = network_item(network_id);
    VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap()
}

fn observation(tick: u64, item_id: i32) -> FrozenCreativeMining {
    FrozenCreativeMining {
        frame: FrozenMiningFrame {
            session_generation: 7,
            position_authority_generation: 2,
            input_authority_generation: NonZeroU64::new(5).unwrap(),
            input_frame_sequence: 31,
            fifo_sequence: 19,
            physics_tick: tick,
            pose_generation: 23,
        },
        ray: FrozenMiningRay {
            origin: [0.5, 65.62, 0.5],
            direction: [0.0, 0.0, -1.0],
            movement_world_identity: world_identity(3),
            world_identity: world_identity(3),
        },
        reach: 5.7,
        input_mode: PlayerInputMode::Mouse,
        ability: CreativeMiningAbility::InstantBreak,
        selection: FrozenMiningSelection {
            slot: 2,
            item: verified_item(item_id),
        },
        target: FrozenMiningTarget {
            position: [0, 64, -2],
            face: 3,
            relative_hit: [0.5, 0.75, 1.0],
            runtime_id: 9,
            identity: world_identity(3),
        },
    }
}

fn completed(tick: u64) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick,
        position: [0.5, 65.620_01, 0.5],
        velocity: [0.0; 3],
        move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 180.0,
        head_yaw: 180.0,
        camera_orientation: [0.0, 0.0, -1.0],
        jumping: false,
        sneaking: false,
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: true,
        grounded_after_tick: true,
        horizontal_collision: false,
        vertical_collision: false,
        jump_repeated: false,
        processed: ProcessedMovementState::default(),
        world_identity: world_identity(3),
    }
}

fn ticker_with_tick() -> MovementTicker {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.5, 65.620_01, 0.5]);
    ticker.set_source(MovementSource::Physics);
    ticker.testing_lift_spawn_settle_gate();
    ticker.enqueue_completed_physics(completed(101)).unwrap();
    ticker
}

fn synthetic_preg(breg: &[u8], records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PREG1001");
    bytes
        .extend_from_slice(&crate::asset_startup::active_content_registry_protocol().to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(u8::try_from(record.collision_seed.boxes.len()).unwrap());
        bytes.push(if record.collision_seed.boxes.is_empty() {
            BlockPhysicsFlags::PASSABLE.bits()
        } else {
            0
        });
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&60_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        for shape in &record.collision_seed.boxes {
            for coordinate in [
                shape.min_x,
                shape.min_y,
                shape.min_z,
                shape.max_x,
                shape.max_y,
                shape.max_z,
            ] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
    }
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

fn fixture_registries() -> PhysicsCollisionRegistries {
    let breg = include_bytes!("../../../crates/assets/data/block-registry-v2168.bin");
    let records = read_registry_for_protocol(breg, 2168).unwrap();
    let preg = synthetic_preg(breg, &records);
    PhysicsCollisionRegistries::from_assets(
        breg,
        &records,
        &preg,
        crate::asset_startup::active_content_registry_protocol(),
    )
    .unwrap()
}

fn production_context_app(
    menu_visible: bool,
    window_focused: bool,
    ticker: MovementTicker,
    runtime: BlockUseRuntime,
) -> App {
    let mut app = App::new();
    app.world_mut().spawn((
        Window {
            focused: window_focused,
            ..Window::default()
        },
        PrimaryWindow,
    ));
    app.insert_resource(SemanticInputSnapshot::default())
        .insert_resource(crate::local_player::InteractionOriginSnapshot::default())
        .insert_resource(UiRuntime::new(7))
        .insert_resource(crate::menu::MenuRuntime::new(
            menu_visible,
            2,
            "Player".to_owned(),
        ))
        .insert_resource(crate::runtime::world::ClientWorld::default())
        .insert_resource(fixture_registries())
        .insert_resource(runtime)
        .insert_resource(ticker)
        .add_systems(Update, super::produce_empty_hand_block_use);
    app
}

fn evidence() -> PhysicsTickEvidenceContext {
    PhysicsTickEvidenceContext {
        fifo_sequence: 19,
        pose_generation: 23,
        dimension: 0,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        look_delta: [0.0; 2],
        outbound_authorized: true,
        outbox_depth: 1,
        outbox_drops: 0,
        free_camera_packet_count: 0,
    }
}

fn pure_movement_wire() -> Vec<u8> {
    let mut ticker = ticker_with_tick();
    let mut bytes = None;
    flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |_identity, packet| {
        bytes = Some(
            protocol::encode(&packet, &BedrockSession { shield_item_id: 0 })
                .unwrap()
                .to_vec(),
        );
        Ok::<_, &str>(())
    })
    .unwrap();
    bytes.unwrap()
}

fn inventory_slot(slot: u8, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(0),
                slot_type: None,
                dynamic_id: None,
            },
            slot: u16::from(slot),
        },
        stack,
        storage_item: None,
    })
}

fn admit_personal_inventory(runtime: &mut UiRuntime) {
    assert!(runtime.inventory_ledger_mut().request_personal_open(42));
    assert!(runtime.inventory_ledger_mut().mark_transport_enqueued(0));
}

#[test]
fn only_one_uncontested_pressed_edge_is_authorized() {
    assert!(block_use_edge_authorized(true, false));
    assert!(!block_use_edge_authorized(false, false));
    assert!(!block_use_edge_authorized(false, true));
    assert!(!block_use_edge_authorized(true, true));
    assert!(mining_edge_authorized(true, false));
    assert!(!mining_edge_authorized(true, true));
}

#[test]
fn only_mouse_creative_empty_hand_observation_enters_the_provisional_path() {
    assert!(FrozenEmptyHandBlockUse::from_observation(observation(101, 0)).is_some());
    assert!(FrozenEmptyHandBlockUse::from_observation(observation(101, 2)).is_none());
    let mut gamepad = observation(101, 0);
    gamepad.input_mode = PlayerInputMode::GamePad;
    assert!(FrozenEmptyHandBlockUse::from_observation(gamepad).is_none());
}

#[test]
fn unknown_nonempty_and_inventory_pending_selection_fail_closed() {
    let mut ui = UiRuntime::new(7);
    ui.publish_player_game_mode(protocol::PlayerGameMode::Creative);
    ui.inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    admit_personal_inventory(&mut ui);
    ui.set_local_selected_slot(2);
    assert!(verified_empty_hand_selection(&ui).is_none());

    ui.inventory_ledger_mut()
        .apply(&inventory_slot(2, NetworkItemStack::empty()));
    assert!(verified_empty_hand_selection(&ui).is_some());

    ui.inventory_ledger_mut()
        .apply(&inventory_slot(2, network_item(2)));
    assert!(verified_empty_hand_selection(&ui).is_none());

    ui.inventory_ledger_mut()
        .apply(&inventory_slot(2, NetworkItemStack::empty()));
    ui.inventory_ledger_mut()
        .apply(&inventory_slot(3, network_item(3)));
    ui.inventory_ledger_mut().begin_click(3).unwrap();
    assert!(ui.inventory_ledger().pending_request_id().is_some());
    assert!(verified_empty_hand_selection(&ui).is_none());

    let mut pending_hotbar = UiRuntime::new(7);
    pending_hotbar.publish_player_game_mode(protocol::PlayerGameMode::Creative);
    pending_hotbar
        .inventory_ledger_mut()
        .apply(&inventory_slot(4, NetworkItemStack::empty()));
    pending_hotbar.queue_local_hotbar_selection(4);
    assert!(pending_hotbar.pending_hotbar_selection().is_some());
    assert!(verified_empty_hand_selection(&pending_hotbar).is_none());

    let mut recovering = UiRuntime::new(7);
    recovering.publish_player_game_mode(protocol::PlayerGameMode::Creative);
    recovering
        .inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    admit_personal_inventory(&mut recovering);
    recovering.set_local_selected_slot(2);
    recovering
        .inventory_ledger_mut()
        .apply(&inventory_slot(2, NetworkItemStack::empty()));
    recovering
        .inventory_ledger_mut()
        .apply(&inventory_slot(3, network_item(3)));
    recovering.inventory_ledger_mut().begin_click(3).unwrap();
    assert!(recovering.inventory_ledger_mut().mark_transport_enqueued(0));
    recovering.inventory_ledger_mut().poll_timeout(u64::MAX);
    assert!(recovering.inventory_ledger().resync_required());
    assert!(verified_empty_hand_selection(&recovering).is_none());
}

#[test]
fn one_edge_attaches_one_use_to_the_exact_pai_tick_without_repeat() {
    let frozen = FrozenEmptyHandBlockUse::from_observation(observation(101, 0)).unwrap();
    let mut ticker = ticker_with_tick();
    let mut runtime = BlockUseRuntime::default();
    assert_eq!(
        runtime.update_press(
            true,
            NonZeroU64::new(5).unwrap(),
            Some(frozen.clone()),
            &mut ticker
        ),
        Some(101)
    );
    assert!(ticker.has_queued_block_use());
    assert_eq!(
        runtime.update_press(
            false,
            NonZeroU64::new(5).unwrap(),
            Some(frozen),
            &mut ticker
        ),
        None
    );

    let mut packet = None;
    assert_eq!(
        flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |_identity, value| {
            packet = Some(value);
            Ok::<_, &str>(())
        })
        .unwrap(),
        1
    );
    let encoded =
        protocol::encode(&packet.unwrap(), &BedrockSession { shield_item_id: 0 }).unwrap();
    assert!(!encoded.is_empty());
}

#[test]
fn transport_retry_is_byte_identical_and_revocation_sanitizes_to_movement_only() {
    let frozen = FrozenEmptyHandBlockUse::from_observation(observation(101, 0)).unwrap();
    let mut ticker = ticker_with_tick();
    let mut runtime = BlockUseRuntime::default();
    runtime.update_press(true, NonZeroU64::new(5).unwrap(), Some(frozen), &mut ticker);
    let session = BedrockSession { shield_item_id: 0 };
    let mut first = None;
    assert!(
        flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |_identity, packet| {
            first = Some(protocol::encode(&packet, &session).unwrap());
            Err("full")
        })
        .is_err()
    );

    let mut admitted = None;
    flush_player_auth_inputs_guarded(
        &mut ticker,
        1,
        Some(evidence()),
        |_identity, packet, guard| {
            let bytes = protocol::encode(&packet, &session).unwrap();
            admitted = Some((packet, guard.unwrap(), bytes));
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    let (packet, guard, retry) = admitted.unwrap();
    assert_eq!(first, Some(retry));

    let mut ui = UiRuntime::new(7);
    ui.publish_player_game_mode(protocol::PlayerGameMode::Creative);
    ui.inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    admit_personal_inventory(&mut ui);
    ui.inventory_ledger_mut()
        .apply(&inventory_slot(4, NetworkItemStack::empty()));
    ui.queue_local_hotbar_selection(4);
    assert!(verified_empty_hand_selection(&ui).is_none());
    runtime.update_press(false, NonZeroU64::new(5).unwrap(), None, &mut ticker);
    assert!(!guard.is_current());
    assert_eq!(
        protocol::encode(&guard.sanitize(packet), &session)
            .unwrap()
            .to_vec(),
        pure_movement_wire()
    );
}

#[test]
fn queued_use_is_stripped_when_inventory_enters_authoritative_recovery() {
    let frozen = FrozenEmptyHandBlockUse::from_observation(observation(101, 0)).unwrap();
    let mut ticker = ticker_with_tick();
    let mut runtime = BlockUseRuntime::default();
    runtime.update_press(true, NonZeroU64::new(5).unwrap(), Some(frozen), &mut ticker);
    let mut admitted = None;
    flush_player_auth_inputs_guarded(
        &mut ticker,
        1,
        Some(evidence()),
        |_identity, packet, guard| {
            admitted = Some((packet, guard.unwrap()));
            Ok::<_, &str>(())
        },
    )
    .unwrap();

    let mut ui = UiRuntime::new(7);
    ui.publish_player_game_mode(protocol::PlayerGameMode::Creative);
    ui.set_local_selected_slot(2);
    ui.inventory_ledger_mut()
        .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    admit_personal_inventory(&mut ui);
    ui.inventory_ledger_mut()
        .apply(&inventory_slot(2, NetworkItemStack::empty()));
    ui.inventory_ledger_mut()
        .apply(&inventory_slot(3, network_item(3)));
    ui.inventory_ledger_mut().begin_click(3).unwrap();
    assert!(ui.inventory_ledger_mut().mark_transport_enqueued(0));
    ui.inventory_ledger_mut().poll_timeout(u64::MAX);
    assert!(ui.inventory_ledger().resync_required());
    assert!(verified_empty_hand_selection(&ui).is_none());

    runtime.update_press(false, NonZeroU64::new(5).unwrap(), None, &mut ticker);
    let (packet, guard) = admitted.unwrap();
    assert!(!guard.is_current());
    assert_eq!(
        protocol::encode(
            &guard.sanitize(packet),
            &BedrockSession { shield_item_id: 0 }
        )
        .unwrap()
        .to_vec(),
        pure_movement_wire()
    );
}

#[test]
fn production_pause_or_focus_loss_sanitizes_an_admitted_use() {
    for (menu_visible, window_focused) in [(true, true), (false, false)] {
        let frozen = FrozenEmptyHandBlockUse::from_observation(observation(101, 0)).unwrap();
        let mut ticker = ticker_with_tick();
        let mut runtime = BlockUseRuntime::default();
        runtime.update_press(true, NonZeroU64::new(5).unwrap(), Some(frozen), &mut ticker);
        let mut admitted = None;
        flush_player_auth_inputs_guarded(
            &mut ticker,
            1,
            Some(evidence()),
            |_identity, packet, guard| {
                admitted = Some((packet, guard.unwrap()));
                Ok::<_, &str>(())
            },
        )
        .unwrap();

        let mut app = production_context_app(menu_visible, window_focused, ticker, runtime);
        app.update();
        let (packet, guard) = admitted.unwrap();
        assert!(!guard.is_current());
        assert_eq!(
            protocol::encode(
                &guard.sanitize(packet),
                &BedrockSession { shield_item_id: 0 }
            )
            .unwrap()
            .to_vec(),
            pure_movement_wire()
        );
    }
}

#[test]
fn use_payload_is_mutually_exclusive_with_destroy() {
    let frozen = FrozenEmptyHandBlockUse::from_observation(observation(101, 0)).unwrap();
    let payload = frozen.into_tick_payload(completed(101).position);
    assert!(payload.interactions.block_actions.is_empty());
    assert!(matches!(
        payload.interactions.block_interaction,
        Some(BlockItemInteraction::Use(_))
    ));
}

#[test]
fn changed_target_or_position_authority_revokes_queued_use() {
    let frozen = FrozenEmptyHandBlockUse::from_observation(observation(101, 0)).unwrap();
    let mut ticker = ticker_with_tick();
    assert_eq!(ticker.attach_block_use(frozen.clone()), Some(101));
    assert!(ticker.has_queued_block_use());

    let mut changed_target = frozen.clone();
    changed_target.observation.target.position[0] += 1;
    ticker.retain_block_use(Some(&changed_target));
    assert!(!ticker.has_queued_block_use());

    let mut ticker = ticker_with_tick();
    assert_eq!(ticker.attach_block_use(frozen), Some(101));
    ticker.reset(8, 101, [0.5, 65.620_01, 0.5]);
    assert!(!ticker.has_queued_block_use());
}
