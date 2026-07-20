use std::cell::Cell;

use super::*;

#[test]
fn packet_id_trace_incremental_drains_are_lifetime_bounded_with_one_terminal_overflow() {
    let mut trace = PacketIdTraceState::default();
    trace.begin();
    trace.observe(McpePacketName::PacketStartGame);
    let first = trace
        .drain()
        .expect("first observed ID is drained promptly");
    assert_eq!(
        first.packet_ids.as_ref(),
        &[McpePacketName::PacketStartGame as u32]
    );
    assert_eq!(first.overflow, 0);
    assert!(!first.timed_out);

    for _ in 1..MAX_PACKET_ID_TRACE_ENTRIES {
        trace.observe(McpePacketName::PacketStartGame);
    }
    let remainder = trace.drain().expect("remaining bounded IDs are drainable");
    assert_eq!(remainder.packet_ids.len(), MAX_PACKET_ID_TRACE_ENTRIES - 1);
    assert!(
        remainder
            .packet_ids
            .iter()
            .all(|id| *id == McpePacketName::PacketStartGame as u32)
    );
    assert_eq!(remainder.overflow, 0);
    assert!(!remainder.timed_out);

    for _ in 0..7 {
        trace.observe(McpePacketName::PacketCommandOutput);
    }
    assert!(
        trace.drain().is_none(),
        "overflow alone must not emit marker spam"
    );

    trace.started_at = Some(std::time::Instant::now() - PACKET_ID_TRACE_DURATION);
    trace.observe(McpePacketName::PacketCommandOutput);
    let terminal = trace.drain().expect("timeout is reported once");
    assert!(terminal.packet_ids.is_empty());
    assert_eq!(terminal.overflow, 7);
    assert!(terminal.timed_out);
    assert!(trace.drain().is_none());
}

#[test]
fn packet_id_trace_cancel_discards_arm_and_all_pending_evidence() {
    let mut trace = PacketIdTraceState::default();
    trace.begin();
    trace.observe(McpePacketName::PacketStartGame);
    trace.cancel();

    assert!(trace.started_at.is_none());
    assert_eq!(trace.recorded, 0);
    assert_eq!(trace.overflow, 0);
    assert!(!trace.timed_out);
    assert!(trace.drain().is_none());
}
use crate::WorldEvent;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::codec::Nbt;
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_30::{
    AddEntityPacket, AddPlayerPacket, AnimateEntityPacket, AnimatePacket, AnimatePacketActionId,
    BiomeDefinition, BiomeDefinitionListPacket, BlockCoordinates, BlockEntityDataPacket,
    CorrectPlayerMovePredictionPacket, GameMode, GameRuleI32, GameRuleI32Type, GameRuleI32Value,
    GameRulesChangedPacket, ItemNew, ItemRegistryPacket, LevelChunkPacket, LevelChunkPacketBlobs,
    LevelEventPacket, LevelEventPacketEvent, McpePacketName, MobEquipmentPacket, MovePlayerPacket,
    SetDefaultGameTypePacket, SetTimePacket, TextPacket, TextPacketCategory, TextPacketContent,
    TextPacketContentJson, TextPacketType, UpdateBlockPacket, UpdatePlayerGameTypePacket, Vec2F,
    Vec3F, WindowId,
};

fn raw_packet(id: McpePacketName, body: &[u8]) -> jolyne::raw::RawPacket {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, id as u32);
    payload.put_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.put_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw packet")
}

#[test]
fn transfer_resets_pending_cache_transactions_but_change_dimension_is_ordered() {
    let cache = ClientBlobCache::default();
    let mut resolver = BlobCacheResolver::new(cache);
    let missing = crate::client_blob_hash(b"missing");
    resolver
        .accept_cached_packet(
            LevelChunkPacket {
                sub_chunk_count: 0,
                blobs: Some(LevelChunkPacketBlobs {
                    hashes: vec![missing],
                }),
                ..Default::default()
            }
            .into(),
        )
        .expect("pending cached column");

    assert!(!reset_cache_for_immediate_boundary(
        &mut resolver,
        McpePacketName::PacketChangeDimension
    ));
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(reset_cache_for_immediate_boundary(
        &mut resolver,
        McpePacketName::PacketTransfer
    ));
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_resets, 1);
}

#[test]
fn fast_transfer_arm_is_consumed_only_after_a_chunk_candidate_decodes() {
    let missing = crate::client_blob_hash(b"old-backend-missing");
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(
            LevelChunkPacket {
                sub_chunk_count: 0,
                blobs: Some(LevelChunkPacketBlobs {
                    hashes: vec![missing],
                }),
                ..Default::default()
            }
            .into(),
        )
        .expect("old unresolved transaction");
    resolver.arm_fast_transfer_rotation();

    let session = BedrockSession { shield_item_id: 0 };
    let malformed = raw_packet(McpePacketName::PacketLevelChunk, &[0xff]);
    assert!(malformed.decode(&session).is_err());
    assert_eq!(resolver.stats().pending_transactions, 1);

    let ordinary: crate::Packet = SetTimePacket { time: 7 }.into();
    assert!(
        !rotate_blob_cache_for_decoded_candidate(&mut resolver, &ordinary)
            .expect("ordinary decoded packet is not a candidate")
    );
    assert_eq!(resolver.stats().pending_transactions, 1);

    let candidate: crate::Packet = LevelChunkPacket {
        sub_chunk_count: 0,
        blobs: None,
        ..Default::default()
    }
    .into();
    assert!(
        rotate_blob_cache_for_decoded_candidate(&mut resolver, &candidate)
            .expect("successfully decoded candidate consumes the arm")
    );
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(
        !rotate_blob_cache_for_decoded_candidate(&mut resolver, &candidate)
            .expect("arm is one-shot")
    );
}

#[test]
fn ignored_play_packet_is_not_materialized() {
    let raw = raw_packet(McpePacketName::PacketNetworkSettings, &[0x7f]);
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        raw.decode(&BedrockSession { shield_item_id: 0 })
    })
    .expect("ignored packet");

    assert!(event.is_none());
    assert!(!decoder_called.get());
}

#[test]
fn allowlisted_ui_packet_is_validated_decoded_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = TextPacket {
        category: TextPacketCategory::MessageOnly,
        type_: TextPacketType::Raw,
        content: Some(TextPacketContent::Raw(TextPacketContentJson {
            message: "live UI".to_owned(),
        })),
        ..Default::default()
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode text");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw text");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode UI event");

    assert!(decoder_called.get());
    assert!(matches!(
        event,
        Some(WorldEvent::Ui(crate::UiEvent::Text(_)))
    ));
}

#[test]
fn live_ui_path_rejects_invalid_utf8_before_owned_decoder() {
    let raw = raw_packet(McpePacketName::PacketText, &[0, 0, 0, 1, 0xff, 0, 0, 0]);
    let decoder_called = Cell::new(false);

    let error = decode_world_raw_with(raw, 0, |_| {
        decoder_called.set(true);
        panic!("invalid UI bytes must fail before owned decoding")
    })
    .expect_err("invalid UI UTF-8");

    assert!(!decoder_called.get());
    assert!(matches!(
        error,
        ProtocolError::World(crate::WorldPacketError::Ui(
            crate::UiPacketError::InvalidUtf8 {
                field: "text.message"
            }
        ))
    ));
}

#[test]
fn live_score_path_checks_text_bound_before_owned_decoder() {
    let mut body = BytesMut::new();
    body.put_u8(0);
    wire::write_var_u32(&mut body, 1);
    wire::write_var_u64(&mut body, 0);
    wire::write_var_u32(&mut body, (crate::MAX_UI_TEXT_BYTES + 1) as u32);
    body.put_bytes(b'x', crate::MAX_UI_TEXT_BYTES + 1);
    let raw = raw_packet(McpePacketName::PacketSetScore, &body);
    let decoder_called = Cell::new(false);

    let error = decode_world_raw_with(raw, 0, |_| {
        decoder_called.set(true);
        panic!("oversized score text must fail before owned decoding")
    })
    .expect_err("oversized score text");

    assert!(!decoder_called.get());
    assert!(matches!(
        error,
        ProtocolError::World(crate::WorldPacketError::Ui(
            crate::UiPacketError::TextTooLong { .. }
        ))
    ));
}

#[test]
fn live_ui_semantic_rejection_is_skippable_world_error() {
    // A well-formed Text packet whose category byte is outside the known set is
    // semantically odd, not malformed wire. The leniency contract requires it
    // to be routed as a skippable world packet (so `skip_or_fail_world` keeps
    // the session alive), never as a fatal error that disconnects the client.
    // Body: needs_translation=false (0x00), category=3 (unknown, > 2).
    let raw = raw_packet(McpePacketName::PacketText, &[0x00, 0x03]);
    let decoder_called = Cell::new(false);

    let error = decode_world_raw_with(raw, 0, |_| {
        decoder_called.set(true);
        panic!("unknown UI category must fail before owned decoding")
    })
    .expect_err("unknown text category");

    assert!(!decoder_called.get());
    assert!(
        matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Ui(
                crate::UiPacketError::UnknownEnum {
                    kind: "text category",
                    value: 3,
                }
            ))
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn live_inventory_content_checks_slot_count_before_owned_decoder() {
    let mut body = BytesMut::new();
    wire::write_var_u32(&mut body, 0);
    wire::write_var_u32(&mut body, (crate::MAX_CONTAINER_SLOTS + 1) as u32);
    let raw = raw_packet(McpePacketName::PacketInventoryContent, &body);
    let decoder_called = Cell::new(false);

    let error = decode_world_raw_with(raw, 0, |_| {
        decoder_called.set(true);
        panic!("oversized inventory content must fail before owned decoding")
    })
    .expect_err("oversized inventory content");

    assert!(!decoder_called.get());
    assert!(matches!(
        error,
        ProtocolError::World(crate::WorldPacketError::Inventory(
            crate::InventoryPacketError::TooManySlots { .. }
        ))
    ));
}

#[test]
fn live_stack_response_checks_nested_counts_before_owned_decoder() {
    let cases = [
        (
            raw_packet(
                McpePacketName::PacketItemStackResponse,
                &varint_body((crate::MAX_STACK_RESPONSES + 1) as u32),
            ),
            "responses",
        ),
        (
            raw_packet(
                McpePacketName::PacketItemStackResponse,
                &accepted_response_prefix((crate::MAX_RESPONSE_CONTAINERS + 1) as u32),
            ),
            "containers",
        ),
        (
            raw_packet(
                McpePacketName::PacketItemStackResponse,
                &accepted_response_with_slot_count((crate::MAX_CONTAINER_SLOTS + 1) as u32),
            ),
            "slots",
        ),
    ];

    for (raw, label) in cases {
        let decoder_called = Cell::new(false);
        let error = decode_world_raw_with(raw, 0, |_| {
            decoder_called.set(true);
            panic!("oversized {label} must fail before owned decoding")
        })
        .expect_err(label);
        assert!(!decoder_called.get(), "owned decoder ran for {label}");
        assert!(
            matches!(
                error,
                ProtocolError::World(crate::WorldPacketError::Inventory(_))
            ),
            "unexpected {label} error: {error:?}"
        );
    }
}

#[test]
fn live_inventory_items_check_extra_length_before_owned_decoder() {
    let mut content = BytesMut::new();
    wire::write_var_u32(&mut content, 0);
    wire::write_var_u32(&mut content, 1);
    append_item_v4_prefix(&mut content, (crate::MAX_ITEM_EXTRA_BYTES + 1) as u32);

    let mut slot = BytesMut::new();
    wire::write_var_u32(&mut slot, 0);
    wire::write_var_u32(&mut slot, 0);
    slot.put_u8(0);
    slot.put_u8(0);
    append_item_new_prefix(&mut slot, (crate::MAX_ITEM_EXTRA_BYTES + 1) as u32);

    for (id, body) in [
        (McpePacketName::PacketInventoryContent, content),
        (McpePacketName::PacketInventorySlot, slot),
    ] {
        let raw = raw_packet(id, &body);
        let decoder_called = Cell::new(false);
        let error = decode_world_raw_with(raw, 0, |_| {
            decoder_called.set(true);
            panic!("oversized item extra must fail before owned decoding")
        })
        .expect_err("oversized item extra");
        assert!(!decoder_called.get());
        assert!(matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Inventory(
                crate::InventoryPacketError::ItemExtraTooLarge { .. }
            ))
        ));
    }
}

#[test]
fn canonical_inventory_fixtures_pass_raw_gate_and_owned_normalization() {
    let session = BedrockSession { shield_item_id: 0 };
    for fixture in [
        &include_bytes!("../../fixtures/inventory_content.bin")[..],
        &include_bytes!("../../fixtures/inventory_slot.bin")[..],
        &include_bytes!("../../fixtures/player_hotbar.bin")[..],
        &include_bytes!("../../fixtures/item_stack_response.bin")[..],
    ] {
        let mut batch = Bytes::copy_from_slice(fixture);
        assert_eq!(batch.get_u8(), 0xfe);
        let raw = decode_packet_raw(&mut batch).expect("raw inventory fixture");
        let event = decode_world_raw_with(raw, 0, |raw| raw.decode(&session))
            .expect("canonical inventory fixture")
            .expect("inventory world event");
        assert!(matches!(event, WorldEvent::Inventory(_)));
    }
}

fn varint_body(value: u32) -> BytesMut {
    let mut body = BytesMut::new();
    wire::write_var_u32(&mut body, value);
    body
}

fn accepted_response_prefix(container_count: u32) -> BytesMut {
    let mut body = BytesMut::new();
    wire::write_var_u32(&mut body, 1);
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    wire::write_var_u32(&mut body, container_count);
    body
}

fn accepted_response_with_slot_count(slot_count: u32) -> BytesMut {
    let mut body = accepted_response_prefix(1);
    body.put_u8(0);
    body.put_u8(0);
    wire::write_var_u32(&mut body, slot_count);
    body
}

fn append_item_v4_prefix(body: &mut BytesMut, extra_length: u32) {
    body.put_i16_le(1);
    body.put_u16_le(1);
    wire::write_var_u32(body, 0);
    body.put_u8(0);
    wire::write_var_u32(body, 0);
    wire::write_var_u32(body, extra_length);
}

fn append_item_new_prefix(body: &mut BytesMut, extra_length: u32) {
    body.put_i16_le(1);
    body.put_u16_le(1);
    wire::write_var_u32(body, 0);
    body.put_u8(0);
    wire::write_var_u32(body, 0);
    wire::write_var_u32(body, extra_length);
}

#[test]
fn allowlisted_world_packet_is_decoded_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = UpdateBlockPacket {
        position: BlockCoordinates { x: 17, y: 2, z: -3 },
        block_runtime_id: 99,
        layer: 0,
        ..Default::default()
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode update");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw update");

    let event = decode_world_raw_with(raw, 2, |raw| raw.decode(&session))
        .expect("decode world update")
        .expect("world event");

    let WorldEvent::BlockUpdates(updates) = event else {
        panic!("expected block updates")
    };
    assert_eq!(updates[0].dimension, 2);
    assert_eq!(updates[0].position, [17, 2, -3]);
    assert_eq!(updates[0].network_id, 99);
}

#[test]
fn allowlisted_block_entity_update_preserves_dimension_position_and_exact_nbt() {
    let session = BedrockSession { shield_item_id: 0 };
    let nbt = vec![10, 0, 0];
    let packet: Packet = BlockEntityDataPacket {
        position: BlockCoordinates { x: 17, y: 2, z: -3 },
        nbt: Nbt(Bytes::copy_from_slice(&nbt)),
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode block entity update");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw block entity update");

    let event = decode_world_raw_with(raw, 2, |raw| raw.decode(&session))
        .expect("decode block entity update")
        .expect("world event");

    assert_eq!(
        event,
        WorldEvent::BlockEntityUpdate(crate::BlockEntityUpdateEvent {
            dimension: 2,
            position: [17, 2, -3],
            nbt,
        })
    );
}

#[test]
fn allowlisted_weather_packet_is_decoded_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = LevelEventPacket {
        event: LevelEventPacketEvent::StartRain,
        data: 48_000,
        ..Default::default()
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode weather event");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw weather event");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode weather event");

    assert!(decoder_called.get());
    assert_eq!(
        event,
        Some(WorldEvent::Weather(crate::WeatherUpdateEvent {
            channel: crate::WeatherChannel::Rain,
            level: 1.0,
        }))
    );
}

#[test]
fn allowlisted_daylight_cycle_rule_is_decoded_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = GameRulesChangedPacket {
        rules: vec![GameRuleI32 {
            name: "DoDaylightCycle".to_owned(),
            type_: GameRuleI32Type::Bool,
            value: Some(GameRuleI32Value::Bool(false)),
            ..Default::default()
        }],
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode gamerule event");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw gamerule event");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode gamerule event");

    assert!(decoder_called.get());
    assert_eq!(
        event,
        Some(WorldEvent::DaylightCycle(crate::DaylightCycleUpdateEvent {
            enabled: false
        }))
    );
}

#[test]
fn allowlisted_move_player_is_materialized_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = MovePlayerPacket {
        runtime_id: 42,
        position: Vec3F {
            x: 1.25,
            y: 70.5,
            z: -8.75,
        },
        pitch: 15.0,
        yaw: -120.25,
        ..Default::default()
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode move player");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw move player");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode move player")
    .expect("move player event");

    assert!(decoder_called.get());
    assert_eq!(
        event,
        WorldEvent::MovePlayer(crate::MovePlayerEvent {
            runtime_id: 42,
            position: [1.25, 70.5, -8.75],
            pitch: 15.0,
            yaw: -120.25,
            head_yaw: 0.0,
            mode: crate::MovePlayerMode::Normal,
            on_ground: false,
            teleported: false,
            source_tick: 0,
        })
    );
}

#[test]
fn allowlisted_actor_packet_is_materialized_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = AddEntityPacket {
        unique_id: 9,
        runtime_id: 42,
        entity_type: "minecraft:bee".to_owned(),
        ..Default::default()
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode add entity");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw add entity");

    let event =
        decode_world_raw_with(raw, 2, |raw| raw.decode(&session)).expect("decode actor event");

    assert!(matches!(
        event,
        Some(WorldEvent::Actor(crate::ActorEvent::Spawn(spawn)))
            if spawn.dimension == 2 && spawn.runtime_id == 42
    ));
}

#[test]
fn allowlisted_player_game_mode_authority_is_decoded_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = UpdatePlayerGameTypePacket {
        gamemode: GameMode::Spectator,
        player_unique_id: -9,
        tick: 27,
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode player game-mode authority");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw player game-mode authority");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 2, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode player game-mode authority");

    assert!(
        decoder_called.get(),
        "allowlisted packet must reach decoder"
    );
    assert!(matches!(
        event,
        Some(WorldEvent::Actor(crate::ActorEvent::GameMode(update)))
            if update.unique_id == -9
                && update.game_mode == crate::ActorGameMode::Spectator
                && update.tick == 27
    ));
}

#[test]
fn allowlisted_default_game_mode_authority_is_decoded_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = SetDefaultGameTypePacket {
        gamemode: GameMode::Spectator,
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode default game-mode authority");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw default game-mode authority");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 2, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode default game-mode authority");

    assert!(
        decoder_called.get(),
        "allowlisted packet must reach decoder"
    );
    assert!(matches!(
        event,
        Some(WorldEvent::Actor(crate::ActorEvent::DefaultGameMode(update)))
            if update.game_mode == crate::ActorGameMode::Spectator
    ));
}

#[test]
fn allowlisted_item_and_action_packets_are_materialized_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packets: [Packet; 5] = [
        AddPlayerPacket {
            runtime_id: 42,
            ..Default::default()
        }
        .into(),
        ItemRegistryPacket::default().into(),
        MobEquipmentPacket {
            runtime_entity_id: 42,
            item: ItemNew {
                network_id: 5,
                count: 1,
                ..Default::default()
            },
            slot: 0,
            selected_slot: 0,
            window_id: WindowId::Inventory,
        }
        .into(),
        AnimatePacket {
            runtime_entity_id: 42,
            action_id: AnimatePacketActionId::SwingArm,
            ..Default::default()
        }
        .into(),
        AnimateEntityPacket {
            animation: "animation.test.attack".into(),
            controller: "controller.animation.test".into(),
            runtime_entity_ids: vec![42],
            ..Default::default()
        }
        .into(),
    ];

    for packet in packets {
        let mut batch = crate::encode(&packet, &session).expect("encode item/action packet");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw item/action packet");
        let event = decode_world_raw_with(raw, 0, |raw| raw.decode(&session))
            .expect("decode item/action event");
        assert!(event.is_some());
    }
}

#[test]
fn canonical_empty_mob_equipment_is_materialized_and_normalized() {
    let mut body = BytesMut::new();
    wire::write_var_u64(&mut body, 42);
    body.put_i16_le(0);
    body.put_u16_le(0);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(0);
    body.put_u8(0);
    body.put_i8(0);
    let raw = raw_packet(McpePacketName::PacketMobEquipment, &body);

    let event = decode_world_raw_with(raw, 0, |raw| {
        raw.decode(&BedrockSession { shield_item_id: 0 })
    })
    .expect("decode empty equipment")
    .expect("empty equipment event");

    assert!(matches!(
        event,
        WorldEvent::Equipment(crate::EquipmentEvent { stack, .. }) if stack == crate::NetworkItemStack::empty()
    ));
}

fn raw_nonempty_mob_equipment(extra: &[u8]) -> RawPacket {
    let mut body = BytesMut::new();
    wire::write_var_u64(&mut body, 42);
    body.put_i16_le(5);
    body.put_u16_le(1);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    wire::write_var_u32(&mut body, extra.len() as u32);
    body.put_slice(extra);
    body.put_u8(0);
    body.put_u8(0);
    body.put_i8(0);
    raw_packet(McpePacketName::PacketMobEquipment, &body)
}

fn raw_zero_count_mob_equipment() -> RawPacket {
    // A non-air network id (5) paired with a zero stack count is not a valid
    // item: it is neither the empty stack nor a real one.
    let mut body = BytesMut::new();
    wire::write_var_u64(&mut body, 42);
    body.put_i16_le(5);
    body.put_u16_le(0);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    let extra = [0u8; 10]; // no NBT, no can-place-on/can-destroy entries
    wire::write_var_u32(&mut body, extra.len() as u32);
    body.put_slice(&extra);
    body.put_u8(0);
    body.put_u8(0);
    body.put_i8(0);
    raw_packet(McpePacketName::PacketMobEquipment, &body)
}

#[test]
fn valid_equipment_is_retained_and_invalid_items_are_rejected() {
    // Byte-exact wire canonicalization was intentionally removed: the client
    // only renders retained items, so faithful round-tripping is unnecessary
    // and rejected legitimate servers. Semantic validation is what guards
    // retention now.
    let session = BedrockSession { shield_item_id: 0 };
    let valid_extra = [0; 10];
    let valid = decode_world_raw_with(raw_nonempty_mob_equipment(&valid_extra), 0, |raw| {
        raw.decode(&session)
    })
    .expect("valid equipment wire");
    assert!(matches!(valid, Some(WorldEvent::Equipment(_))));

    let error = decode_world_raw_with(raw_zero_count_mob_equipment(), 0, |raw| {
        raw.decode(&session)
    })
    .expect_err("zero-count item is semantically invalid");
    assert!(
        matches!(
            &error,
            ProtocolError::World(crate::WorldPacketError::Item(
                crate::ItemPacketError::InvalidItemCount
            ))
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn absolute_actor_move_uses_bedrock_varuint_and_raw_byte_rotations() {
    let runtime_id = (u64::from(u32::MAX) + 42) << 1;
    let mut body = BytesMut::new();
    wire::write_var_u64(&mut body, runtime_id);
    body.put_u8(0b11);
    body.put_f32_le(12.5);
    body.put_f32_le(64.25);
    body.put_f32_le(-7.75);
    body.put_u8(32);
    body.put_u8(64);
    body.put_u8(128);
    let raw = raw_packet(McpePacketName::PacketMoveEntity, &body);
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 2, |_| {
        decoder_called.set(true);
        panic!("absolute actor movement must bypass Valentine's incompatible Rotation shape")
    })
    .expect("decode absolute actor move")
    .expect("absolute actor move event");

    assert!(!decoder_called.get());
    assert_eq!(
        event,
        WorldEvent::Actor(crate::ActorEvent::Move(crate::ActorMoveEvent {
            dimension: 2,
            runtime_id,
            position: [Some(12.5), Some(64.25), Some(-7.75)],
            position_origin: crate::ActorPositionOrigin::NetworkOffset,
            pitch: Some(45.0),
            yaw: Some(90.0),
            head_yaw: Some(180.0),
            on_ground: Some(true),
            teleported: true,
            snap: true,
            player_mode: None,
            source_tick: None,
        }))
    );
}

#[test]
fn absolute_actor_move_rejects_truncated_and_trailing_bodies() {
    let mut valid = BytesMut::new();
    wire::write_var_u64(&mut valid, 42);
    valid.put_u8(0);
    valid.put_f32_le(1.0);
    valid.put_f32_le(2.0);
    valid.put_f32_le(3.0);
    valid.put_slice(&[0, 0, 0]);

    for (body, actual) in [
        (&valid[..valid.len() - 1], 15_usize),
        (&[valid.as_ref(), &[0xff]].concat(), 17_usize),
    ] {
        let raw = raw_packet(McpePacketName::PacketMoveEntity, body);
        let error = decode_world_raw_with(raw, 0, |_| {
            panic!("malformed absolute actor movement must bypass Valentine")
        })
        .expect_err("malformed absolute actor move");
        assert!(matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Actor(
                crate::ActorPacketError::InvalidAbsoluteMoveLength {
                    actual: found,
                    expected: 16,
                }
            )) if found == actual
        ));
    }
}

#[test]
fn allowlisted_movement_correction_is_materialized_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = CorrectPlayerMovePredictionPacket {
        position: Vec3F {
            x: 27.5,
            y: 111.0,
            z: 91.5,
        },
        delta: Vec3F {
            x: 0.5,
            y: -0.25,
            z: 1.0,
        },
        rotation: Vec2F { x: -15.0, z: 90.25 },
        on_ground: true,
        tick: 55,
        ..Default::default()
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode movement correction");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw movement correction");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode movement correction")
    .expect("movement correction event");

    assert!(decoder_called.get());
    assert_eq!(
        event,
        WorldEvent::PlayerMovementCorrection(crate::PlayerMovementCorrectionEvent {
            position: [27.5, 111.0, 91.5],
            delta: [0.5, -0.25, 1.0],
            pitch: -15.0,
            yaw: 90.25,
            on_ground: true,
            tick: 55,
        })
    );
}

#[test]
fn allowlisted_biome_definitions_are_materialized_and_normalized() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = BiomeDefinitionListPacket {
        biome_definitions: vec![BiomeDefinition {
            name_index: 0,
            biome_id: u16::MAX,
            temperature: 0.8,
            downfall: 0.4,
            snow_foliage: 0.0,
            map_water_colour: 0xff44_6688_u32 as i32,
            ..Default::default()
        }],
        string_list: vec!["plains".into()],
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode biome definitions");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw biome definitions");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        raw.decode(&session)
    })
    .expect("decode biome definitions");

    assert!(decoder_called.get());
    let WorldEvent::BiomeDefinitions(event) = event.expect("biome definitions event") else {
        panic!("expected biome definitions")
    };
    assert_eq!(event.definitions[0].biome_id, None);
    assert_eq!(event.definitions[0].name.as_ref(), "minecraft:plains");
}
