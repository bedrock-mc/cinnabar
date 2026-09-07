use std::{num::NonZeroU64, sync::Arc};

use assets::{BlockPhysicsFlags, RegistryRecord, read_registry_for_protocol};
use bevy::{
    prelude::{App, Update, Window},
    window::PrimaryWindow,
};
use protocol::{BedrockSession, NetworkItemStack, PlayerInputMode, VerifiedNetworkItemStack};
use sha2::Digest;
use sim::{CollisionIdSpace, CollisionRegistryIdentity, WorldCollisionIdentity};

use super::{
    CreativeMiningAbility, FrozenCreativeMining, FrozenMiningFrame, FrozenMiningRay,
    FrozenMiningSelection, FrozenMiningTarget, MiningRuntime, creative_mining_ability,
    creative_mining_input_authorized, creative_mining_ui_ability, creative_reach,
};
use crate::movement::{
    MovementSource, MovementTicker, PhysicsCollisionRegistries, PhysicsMovementSample,
    PhysicsTickEvidenceContext, ProcessedMovementState, flush_player_auth_inputs,
    flush_player_auth_inputs_guarded,
};
use crate::{
    local_player::InteractionOriginSnapshot, menu::MenuRuntime, runtime::world::ClientWorld,
    semantic_controls::SemanticInputSnapshot, ui_runtime::UiRuntime,
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

fn cross_chunk_world_identity(revision: u64) -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            protocol: 2168,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [7; 32],
        },
        [
            world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, 0, 0),
                revision,
            },
            world::ChunkCollisionRevision {
                chunk: world::ChunkKey::new(0, 1, 0),
                revision,
            },
        ],
    )
    .unwrap()
}

fn verified_item(network_id: i32) -> VerifiedNetworkItemStack {
    let stack = if network_id == 0 {
        NetworkItemStack::empty()
    } else {
        let extra_data: Arc<[u8]> = Arc::from([]);
        let digest = sha2::Sha256::digest(&extra_data);
        NetworkItemStack {
            network_id,
            metadata: 0,
            stack_network_id: 41,
            count: 1,
            nbt_digest: digest.into(),
            block_runtime_id: 0,
            extra_data,
        }
    };
    VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap()
}

fn observation(tick: u64, target: [i32; 3], item_id: i32) -> FrozenCreativeMining {
    FrozenCreativeMining {
        frame: FrozenMiningFrame {
            session_generation: 7,
            position_authority_generation: 2,
            input_authority_generation: NonZeroU64::new(5).unwrap(),
            fifo_sequence: 19,
            physics_tick: tick,
            pose_generation: 23,
        },
        ray: FrozenMiningRay {
            origin: [0.5, 2.62, 0.5],
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
            position: target,
            face: 3,
            relative_hit: [0.5, 0.25, 1.0],
            runtime_id: 9,
            identity: world_identity(3),
        },
    }
}

fn completed(tick: u64) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick,
        position: [0.5, 2.620_01, 0.5],
        velocity: [0.0; 3],
        move_vector: [0.0; 2],
        raw_move_vector: [0.0; 2],
        analogue_move_vector: [0.0; 2],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
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

fn ticker_with_ticks(count: usize) -> MovementTicker {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.5, 2.620_01, 0.5]);
    ticker.set_source(MovementSource::Physics);
    ticker.testing_lift_spawn_settle_gate();
    for offset in 0..count {
        ticker
            .enqueue_completed_physics(completed(101 + offset as u64))
            .unwrap();
    }
    ticker
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

fn synthetic_preg(breg: &[u8], records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PREG1001");
    bytes
        .extend_from_slice(&crate::asset_startup::active_content_registry_protocol().to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&sha2::Sha256::digest(breg));
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
    let digest = sha2::Sha256::digest(&bytes);
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
    runtime: MiningRuntime,
) -> App {
    let window = Window {
        focused: window_focused,
        ..Window::default()
    };
    let mut app = App::new();
    app.world_mut().spawn((window, PrimaryWindow));
    app.insert_resource(SemanticInputSnapshot::default())
        .insert_resource(InteractionOriginSnapshot::default())
        .insert_resource(UiRuntime::new(7))
        .insert_resource(MenuRuntime::new(menu_visible, 2, "Player".to_owned()))
        .insert_resource(ClientWorld::default())
        .insert_resource(fixture_registries())
        .insert_resource(runtime)
        .insert_resource(ticker)
        .add_systems(Update, super::produce_creative_mining);
    app
}

fn encoded(packet: &protocol::Packet) -> Vec<u8> {
    protocol::encode(packet, &BedrockSession { shield_item_id: 0 })
        .unwrap()
        .to_vec()
}

fn pure_movement_wire() -> Vec<u8> {
    let mut ticker = ticker_with_ticks(1);
    let mut packet = None;
    flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |_identity, movement| {
        packet = Some(encoded(&movement));
        Ok::<_, &str>(())
    })
    .unwrap();
    packet.unwrap()
}

#[test]
fn creative_break_payload_is_complete_and_ordered_before_attachment() {
    let frozen = observation(101, [0, 1, -3], 2);
    let player_position = completed(101).position;
    let payload = frozen.clone().into_tick_payload(player_position);
    let kinds = payload
        .interactions
        .block_actions
        .iter()
        .map(|action| action.kind)
        .collect::<Vec<_>>();

    assert_eq!(
        kinds,
        [
            protocol::BlockActionKind::StartDestroy,
            protocol::BlockActionKind::PredictDestroy,
        ]
    );
    let destroy = payload.interactions.block_destroy.unwrap();
    assert_eq!(destroy.block_position, frozen.target.position);
    assert_eq!(destroy.face, frozen.target.face);
    assert_eq!(destroy.selected_slot, frozen.selection.slot);
    assert_eq!(destroy.selected_item, frozen.selection.item);
    assert_eq!(destroy.player_position, player_position);
    assert_eq!(destroy.relative_hit, frozen.target.relative_hit);
    assert_eq!(
        destroy.block_runtime_id,
        u64::from(frozen.target.runtime_id)
    );
}

#[test]
fn only_explicit_creative_mode_grants_the_first_slice_ability() {
    assert_eq!(
        creative_mining_ability(protocol::PlayerGameMode::Creative),
        Some(CreativeMiningAbility::InstantBreak)
    );
    for mode in [
        protocol::PlayerGameMode::Survival,
        protocol::PlayerGameMode::Adventure,
        protocol::PlayerGameMode::Spectator,
        protocol::PlayerGameMode::Unknown,
    ] {
        assert_eq!(creative_mining_ability(mode), None);
    }
}

#[test]
fn ui_ownership_revokes_even_an_explicit_creative_ability() {
    assert_eq!(
        creative_mining_ui_ability(true, protocol::PlayerGameMode::Creative),
        None
    );
    assert_eq!(
        creative_mining_ui_ability(false, protocol::PlayerGameMode::Creative),
        Some(CreativeMiningAbility::InstantBreak)
    );
}

#[test]
fn menu_and_window_ownership_are_required_for_creative_mining_input() {
    assert!(creative_mining_input_authorized(false, true));
    assert!(!creative_mining_input_authorized(true, true));
    assert!(!creative_mining_input_authorized(false, false));
}

#[test]
fn creative_reach_is_frozen_per_input_mode() {
    assert_eq!(creative_reach(PlayerInputMode::Mouse), 5.7);
    assert_eq!(creative_reach(PlayerInputMode::GamePad), 5.6);
    assert_eq!(creative_reach(PlayerInputMode::Touch), 12.0);
}

#[test]
fn press_waits_for_and_attaches_to_the_exact_frozen_tick() {
    let mut ticker = ticker_with_ticks(0);
    let mut runtime = MiningRuntime::default();
    let authority = NonZeroU64::new(5).unwrap();

    assert_eq!(
        runtime.update_press(
            true,
            authority,
            Some(observation(101, [0, 1, -3], 2)),
            &mut ticker,
        ),
        None
    );
    assert!(runtime.has_pending_press());
    ticker.enqueue_completed_physics(completed(101)).unwrap();

    assert_eq!(
        runtime.update_press(
            false,
            authority,
            Some(observation(101, [0, 1, -3], 2)),
            &mut ticker,
        ),
        Some(101)
    );
    assert!(!runtime.has_pending_press());
    assert!(ticker.has_queued_creative_mining());
}

#[test]
fn transport_full_retry_preserves_the_exact_combined_payload_once() {
    let mut ticker = ticker_with_ticks(1);
    let authority = NonZeroU64::new(5).unwrap();
    let current = observation(101, [0, 1, -3], 2);
    let mut runtime = MiningRuntime::default();
    assert_eq!(
        runtime.update_press(true, authority, Some(current.clone()), &mut ticker),
        Some(101)
    );

    let mut first = None;
    assert!(
        flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |_identity, packet| {
            first = Some(encoded(&packet));
            Err("full")
        })
        .is_err()
    );
    assert!(ticker.has_queued_creative_mining());

    let mut movement_only = ticker_with_ticks(1);
    let mut pure = None;
    assert_eq!(
        flush_player_auth_inputs(
            &mut movement_only,
            1,
            Some(evidence()),
            |_identity, packet| {
                pure = Some(encoded(&packet));
                Ok::<_, &str>(())
            },
        )
        .unwrap(),
        1
    );
    assert_ne!(first, pure);

    runtime.update_press(false, authority, Some(current), &mut ticker);
    let mut retry = None;
    let mut retry_identity = None;
    let sent = flush_player_auth_inputs_guarded(
        &mut ticker,
        1,
        Some(evidence()),
        |identity, packet, guard| {
            retry_identity = Some(identity);
            let guard = guard.expect("the retried combined packet remains guarded");
            assert!(guard.is_current());
            retry = Some(encoded(&guard.sanitize(packet)));
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert_eq!(sent, 1);
    assert_eq!(first, retry);
    assert!(ticker.has_queued_creative_mining());
    assert!(ticker.acknowledge_physics_send(retry_identity.unwrap()));
    assert!(!ticker.has_queued_creative_mining());
}

#[test]
fn saturated_queue_keeps_the_break_on_its_original_tail_tick() {
    let mut ticker = ticker_with_ticks(crate::movement::OUTBOX_CAPACITY);
    let authority = NonZeroU64::new(5).unwrap();
    let tail_tick = 100 + crate::movement::OUTBOX_CAPACITY as u64;
    let mut runtime = MiningRuntime::default();
    let attached = runtime.update_press(
        true,
        authority,
        Some(observation(tail_tick, [0, 1, -3], 2)),
        &mut ticker,
    );

    assert_eq!(attached, Some(tail_tick));
    assert!(ticker.has_queued_creative_mining());
    assert_eq!(ticker.pending_count(), crate::movement::OUTBOX_CAPACITY);
}

#[test]
fn mismatched_tick_collision_identity_cannot_receive_the_break() {
    let mut ticker = ticker_with_ticks(1);
    let authority = NonZeroU64::new(5).unwrap();
    let mut current = observation(101, [0, 1, -3], 2);
    current.ray.movement_world_identity = world_identity(4);
    let mut runtime = MiningRuntime::default();

    assert_eq!(
        runtime.update_press(true, authority, Some(current), &mut ticker),
        None
    );
    assert!(!ticker.has_queued_creative_mining());
    assert_eq!(ticker.pending_count(), 1);
}

#[test]
fn cross_chunk_ray_identity_attaches_to_its_distinct_local_movement_tick() {
    let mut ticker = ticker_with_ticks(1);
    let authority = NonZeroU64::new(5).unwrap();
    let mut current = observation(101, [16, 1, 0], 2);
    current.ray.world_identity = cross_chunk_world_identity(3);
    current.target.identity = current.ray.world_identity.clone();
    let mut runtime = MiningRuntime::default();

    assert_eq!(
        runtime.update_press(true, authority, Some(current), &mut ticker),
        Some(101)
    );
    assert!(ticker.has_queued_creative_mining());
}

#[test]
fn selection_or_target_loss_strips_an_unsent_break_without_dropping_movement() {
    let mut ticker = ticker_with_ticks(1);
    let authority = NonZeroU64::new(5).unwrap();
    let current = observation(101, [0, 1, -3], 2);
    let mut runtime = MiningRuntime::default();
    runtime.update_press(true, authority, Some(current.clone()), &mut ticker);

    let changed_selection = observation(101, [0, 1, -3], 3);
    runtime.update_press(false, authority, Some(changed_selection), &mut ticker);
    assert!(!ticker.has_queued_creative_mining());
    assert_eq!(ticker.pending_count(), 1);

    runtime.update_press(true, authority, Some(current), &mut ticker);
    assert!(ticker.has_queued_creative_mining());
    let changed_target = observation(101, [1, 1, -3], 2);
    runtime.update_press(false, authority, Some(changed_target), &mut ticker);
    assert!(!ticker.has_queued_creative_mining());
    assert_eq!(ticker.pending_count(), 1);
}

#[test]
fn correction_and_session_reset_invalidate_unsent_creative_mining() {
    let authority = NonZeroU64::new(5).unwrap();
    let mut runtime = MiningRuntime::default();
    let mut ticker = ticker_with_ticks(1);
    runtime.update_press(
        true,
        authority,
        Some(observation(101, [0, 1, -3], 0)),
        &mut ticker,
    );
    assert!(ticker.has_queued_creative_mining());

    ticker.reanchor_surface_spawn(101, [8.0, 70.620_01, 8.0]);
    assert!(!ticker.has_queued_creative_mining());
    assert_eq!(
        runtime.update_press(
            true,
            authority,
            Some(observation(101, [0, 1, -3], 0)),
            &mut ticker,
        ),
        None
    );
    assert!(!runtime.has_pending_press());

    let mut ticker = ticker_with_ticks(0);
    let mut runtime = MiningRuntime::default();
    runtime.update_press(
        true,
        authority,
        Some(observation(101, [0, 1, -3], 0)),
        &mut ticker,
    );
    assert!(runtime.has_pending_press());
    ticker.reset(8, 200, [1.0, 65.620_01, 1.0]);
    runtime.update_press(false, authority, None, &mut ticker);
    assert!(!runtime.has_pending_press());
}

#[test]
fn unavailable_current_authority_strips_only_the_unsent_break() {
    let mut ticker = ticker_with_ticks(1);
    let authority = NonZeroU64::new(5).unwrap();
    let mut runtime = MiningRuntime::default();
    runtime.update_press(
        true,
        authority,
        Some(observation(101, [0, 1, -3], 2)),
        &mut ticker,
    );

    runtime.update_press(false, authority, None, &mut ticker);

    assert!(!ticker.has_queued_creative_mining());
    assert_eq!(ticker.pending_count(), 1);
    assert!(!runtime.has_pending_press());
}

#[test]
fn production_pause_or_settings_schedule_strips_a_queued_break_and_resume_accepts_fresh_press() {
    let authority = NonZeroU64::new(5).unwrap();
    let mut ticker = ticker_with_ticks(1);
    let mut runtime = MiningRuntime::default();
    runtime.update_press(
        true,
        authority,
        Some(observation(101, [0, 1, -3], 2)),
        &mut ticker,
    );
    let mut app = production_context_app(true, true, ticker, runtime);

    app.update();
    assert!(
        !app.world()
            .resource::<MovementTicker>()
            .has_queued_creative_mining()
    );
    assert_eq!(app.world().resource::<MovementTicker>().pending_count(), 1);

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_visible(false);
    let mut ticker = app.world_mut().remove_resource::<MovementTicker>().unwrap();
    let mut runtime = app.world_mut().remove_resource::<MiningRuntime>().unwrap();
    ticker.enqueue_completed_physics(completed(102)).unwrap();
    assert_eq!(
        runtime.update_press(
            true,
            authority,
            Some(observation(102, [0, 1, -3], 2)),
            &mut ticker,
        ),
        Some(102)
    );
}

#[test]
fn production_pause_or_focus_loss_sanitizes_an_already_admitted_break() {
    let authority = NonZeroU64::new(5).unwrap();
    for (menu_visible, window_focused) in [(true, true), (false, false)] {
        let mut ticker = ticker_with_ticks(1);
        let mut runtime = MiningRuntime::default();
        runtime.update_press(
            true,
            authority,
            Some(observation(101, [0, 1, -3], 2)),
            &mut ticker,
        );
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
        let (combined, guard) = admitted.unwrap();
        assert!(!guard.is_current());
        assert_eq!(encoded(&guard.sanitize(combined)), pure_movement_wire());
        assert_eq!(app.world().resource::<MovementTicker>().pending_count(), 1);
    }
}

#[test]
fn changed_semantic_input_authority_revokes_an_admitted_break() {
    let authority = NonZeroU64::new(5).unwrap();
    let replacement_authority = NonZeroU64::new(6).unwrap();
    let mut ticker = ticker_with_ticks(1);
    let mut runtime = MiningRuntime::default();
    runtime.update_press(
        true,
        authority,
        Some(observation(101, [0, 1, -3], 2)),
        &mut ticker,
    );
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
    let mut current = observation(101, [0, 1, -3], 2);
    current.frame.input_authority_generation = replacement_authority;

    runtime.update_press(false, replacement_authority, Some(current), &mut ticker);

    let (combined, guard) = admitted.unwrap();
    assert!(!guard.is_current());
    assert_eq!(encoded(&guard.sanitize(combined)), pure_movement_wire());
    assert!(!ticker.has_queued_creative_mining());
    assert_eq!(ticker.pending_count(), 1);
}

#[test]
fn post_admission_ui_ability_selection_target_and_world_revocation_keep_pure_movement() {
    let authority = NonZeroU64::new(5).unwrap();
    let original = observation(101, [0, 1, -3], 2);
    let mut changed_selection = original.clone();
    changed_selection.selection.item = verified_item(3);
    let mut changed_target = original.clone();
    changed_target.target.position = [1, 1, -3];
    let mut changed_world = original.clone();
    changed_world.ray.world_identity = world_identity(4);

    let mut pure_ticker = ticker_with_ticks(1);
    let mut pure = None;
    flush_player_auth_inputs(
        &mut pure_ticker,
        1,
        Some(evidence()),
        |_identity, packet| {
            pure = Some(encoded(&packet));
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    let pure = pure.unwrap();

    for current in [
        None,
        Some(changed_selection),
        Some(changed_target),
        Some(changed_world),
    ] {
        let mut ticker = ticker_with_ticks(1);
        let mut runtime = MiningRuntime::default();
        runtime.update_press(true, authority, Some(original.clone()), &mut ticker);
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
        assert!(admitted.as_ref().unwrap().1.is_current());

        runtime.update_press(false, authority, current, &mut ticker);
        let (combined, guard) = admitted.unwrap();
        assert!(!guard.is_current());
        assert_eq!(encoded(&guard.sanitize(combined)), pure);
        assert!(!ticker.has_queued_creative_mining());
        assert_eq!(ticker.pending_count(), 1);
    }
}

#[test]
fn correction_cancellation_does_not_retry_an_accepted_stale_break() {
    let mut ticker = ticker_with_ticks(1);
    let authority = NonZeroU64::new(5).unwrap();
    let current = observation(101, [0, 1, -3], 2);
    let mut runtime = MiningRuntime::default();
    runtime.update_press(true, authority, Some(current.clone()), &mut ticker);

    let mut first_identity = None;
    let mut first = None;
    assert_eq!(
        flush_player_auth_inputs(&mut ticker, 1, Some(evidence()), |identity, packet| {
            first_identity = Some(identity);
            first = Some(encoded(&packet));
            Ok::<_, &str>(())
        })
        .unwrap(),
        1
    );
    assert!(first.is_some());
    ticker.reanchor_surface_spawn(101, [8.0, 70.620_01, 8.0]);
    runtime.update_press(false, authority, Some(current), &mut ticker);
    assert!(ticker.resolve_cancelled_physics_send(first_identity.unwrap(), true));
    assert!(!ticker.has_queued_creative_mining());
    assert_eq!(
        flush_player_auth_inputs(
            &mut ticker,
            1,
            Some(evidence()),
            |_identity, _packet| -> Result<(), &'static str> {
                panic!("the stale mining tick must not be retried")
            },
        )
        .unwrap(),
        0
    );
}
