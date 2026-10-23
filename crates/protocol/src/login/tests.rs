use std::cell::Cell;
use std::sync::Arc;

use super::*;
use crate::BlobCacheReady;

use crate::WorldEvent;
use crate::raw_packet::decode_packet_raw;
use bedrock_protocol_1_26_51::enums::{AnimateAction, PlayerRespawnState, TextPacketType};
use bedrock_protocol_1_26_51::packets::{
    AddActor, AddPlayer, Animate, AnimateEntity, BiomeDefinitionList, BlockActorData,
    ClientCacheMissResponse, CorrectPlayerMovePrediction, DeathInfo, GameRulesChanged, LevelChunk,
    LevelEvent, MobEquipment, MovePlayer, PlaySound, Respawn, SetTime, Text, UpdateBlock,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, ActorUniqueID, BiomeDefinitionData, BiomeStringList, BlockPos, GameRule,
    GameRuleValue, GameRulesChangedData, MissingBlobData, NetworkItemStackDescriptorSerializedData,
    PlayerInputTick, SubChunkMetadata, TextData,
};
use bytes::{Buf, BufMut, Bytes};

mod container_registry_cleanup;

#[test]
fn all_named_audio_packets_are_allowlisted_for_raw_world_ingress() {
    for fixture in [
        include_bytes!("../../fixtures/play_sound.bin").as_slice(),
        include_bytes!("../../fixtures/stop_sound.bin").as_slice(),
        include_bytes!("../../fixtures/level_sound_event.bin").as_slice(),
    ] {
        let mut batch = Bytes::copy_from_slice(fixture);
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw audio fixture");
        let event = decode_world_raw_with(raw, 0, super::decode_test_raw)
            .expect("audio wire decode")
            .expect("allowlisted audio event");
        assert!(matches!(event, WorldEvent::Audio(_)));
    }
}

#[test]
fn audio_semantic_rejections_are_skippable_world_errors() {
    let packet: Packet = crate::Packet::new(PlaySound {
        name: "x".repeat(crate::MAX_AUDIO_IDENTIFIER_BYTES + 1).into(),
        volume: wire::F32LE(1.0),
        pitch: wire::F32LE(1.0),
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode overlong semantic packet");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw overlong packet");
    let error = decode_world_raw_with(raw, 0, super::decode_test_raw)
        .expect_err("overlong identifier is semantically unusable");
    assert!(matches!(
        error,
        ProtocolError::World(crate::WorldPacketError::AudioIdentifierTooLong { .. })
    ));

    let packet: Packet = crate::Packet::new(PlaySound {
        volume: wire::F32LE(f32::NAN),
        pitch: wire::F32LE(1.0),
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode non-finite semantic packet");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw non-finite packet");
    assert!(matches!(
        decode_world_raw_with(raw, 0, super::decode_test_raw),
        Err(ProtocolError::World(
            crate::WorldPacketError::NonFiniteAudioField { .. }
        ))
    ));
}

#[test]
fn truncated_audio_wire_remains_fatal() {
    let raw = raw_packet(PacketId::PlaySound, &[2, b'x']);
    assert!(matches!(
        decode_world_raw_with(raw, 0, super::decode_test_raw),
        Err(ProtocolError::Session(_))
    ));
}

/// Builds a cache-enabled LevelChunk carrying exactly the given blob hashes.
///
/// 1.26.40 writes the hashes unconditionally and signals cache participation
/// with `cache_enabled`, so `blobs: Some(..)` has no direct equivalent.
pub(super) fn cached_level_chunk(hashes: Vec<u64>) -> LevelChunk {
    let sub_chunks_count = u32::try_from(hashes.len().saturating_sub(1)).expect("fixture count");
    LevelChunk {
        sub_chunks_count: wire::VarUInt(sub_chunks_count),
        cache_enabled: true,
        cache_metadata: hashes.into_iter().map(SubChunkMetadata).collect(),
        ..Default::default()
    }
}

fn raw_packet(id: PacketId, body: &[u8]) -> crate::raw_packet::RawPacket {
    let mut payload = Vec::new();
    wire::Writer::new(&mut payload)
        .write_var_u32(id as u32)
        .unwrap();
    payload.put_slice(body);
    let mut frame = Vec::new();
    wire::Writer::new(&mut frame)
        .write_var_u32(payload.len() as u32)
        .unwrap();
    frame.put_slice(&payload);
    decode_packet_raw(&mut Bytes::from(frame)).expect("raw packet")
}

#[test]
fn transfer_resets_pending_cache_transactions_but_change_dimension_is_ordered() {
    let cache = ClientBlobCache::default();
    let mut resolver = BlobCacheResolver::new(cache);
    let missing = crate::client_blob_hash(b"missing");
    resolver
        .accept_cached_packet(crate::Packet::new(cached_level_chunk(vec![missing])))
        .expect("pending cached column");

    assert!(
        !reset_cache_for_immediate_boundary(&mut resolver, PacketId::ChangeDimension)
            .expect("change dimension does not reset immediately")
    );
    assert_eq!(resolver.stats().pending_transactions, 1);
    assert!(
        reset_cache_for_immediate_boundary(&mut resolver, PacketId::Transfer)
            .expect("transfer preserves rollback recovery")
    );
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert_eq!(resolver.stats().pending_resets, 1);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::WorldEvent(WorldEvent::ChunkResync(_)))
    ));
}

#[test]
fn unrelated_world_semantic_skip_does_not_recover_pending_cached_terrain() {
    let cache = ClientBlobCache::default();
    let missing_payload = b"late terrain blob";
    let missing = crate::client_blob_hash(missing_payload);
    let mut resolver = BlobCacheResolver::new(cache);
    resolver
        .accept_cached_packet(crate::Packet::new(cached_level_chunk(vec![missing])))
        .expect("pending cached column");
    assert_eq!(resolver.stats().pending_transactions, 1);

    let mut world_skips = 0;
    skip_semantic_world_error(
        ProtocolError::World(crate::WorldPacketError::Ui(
            crate::UiPacketError::UnknownEnum {
                kind: "text category",
                value: 3,
            },
        )),
        &mut world_skips,
    )
    .expect("unrelated semantic rejection is skipped");
    assert_eq!(world_skips, 1);
    assert_eq!(resolver.stats().pending_transactions, 1);

    resolver
        .accept_miss_response(ClientCacheMissResponse {
            missing_blobs: vec![MissingBlobData {
                blob_id: wire::U64LE(missing),
                blob_data: Bytes::from_static(missing_payload),
            }],
        })
        .expect("late cache miss response resolves normally");
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(matches!(
        resolver.pop_ready(),
        Some(BlobCacheReady::Packet(packet))
            if matches!(*packet.data, packets::Packet::LevelChunk(_))
    ));
}

#[test]
fn fatal_session_reset_clears_pending_without_clearing_verified_entries() {
    let cache = ClientBlobCache::default();
    let verified = cache
        .insert(b"verified terrain")
        .expect("seed verified blob");
    let missing = crate::client_blob_hash(b"missing terrain");
    let mut resolver = BlobCacheResolver::new(cache.clone());
    resolver
        .accept_cached_packet(crate::Packet::new(cached_level_chunk(vec![missing])))
        .expect("pending cached column");

    resolver.reset_pending();

    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(cache.contains(verified));
}

#[test]
fn fast_transfer_arm_is_consumed_only_after_a_chunk_candidate_decodes() {
    let missing = crate::client_blob_hash(b"old-backend-missing");
    let mut resolver = BlobCacheResolver::new(ClientBlobCache::default());
    resolver
        .accept_cached_packet(crate::Packet::new(cached_level_chunk(vec![missing])))
        .expect("old unresolved transaction");
    resolver.arm_fast_transfer_reset();

    let malformed = raw_packet(PacketId::LevelChunk, &[0xff]);
    assert!(super::decode_test_raw(malformed).is_err());
    assert_eq!(resolver.stats().pending_transactions, 1);

    let ordinary: crate::Packet = crate::Packet::new(SetTime {
        time: wire::ZigZag32(7),
    });
    assert!(
        !reset_blob_cache_for_decoded_candidate(&mut resolver, &ordinary)
            .expect("ordinary decoded packet is not a candidate")
    );
    assert_eq!(resolver.stats().pending_transactions, 1);

    let candidate: crate::Packet = crate::Packet::new(LevelChunk {
        sub_chunks_count: wire::VarUInt(0),
        cache_enabled: false,
        ..Default::default()
    });
    assert!(
        reset_blob_cache_for_decoded_candidate(&mut resolver, &candidate)
            .expect("successfully decoded candidate consumes the arm")
    );
    assert_eq!(resolver.stats().pending_transactions, 0);
    assert!(
        !reset_blob_cache_for_decoded_candidate(&mut resolver, &candidate)
            .expect("arm is one-shot")
    );
}

#[test]
fn malformed_cached_chunk_wire_remains_a_fatal_session_error() {
    let malformed = raw_packet(PacketId::LevelChunk, &[0xff]);

    let error = decode_world_raw_with(malformed, 0, super::decode_test_raw)
        .expect_err("truncated cached LevelChunk wire must fail closed");

    assert!(matches!(error, ProtocolError::Session(_)));
}

#[test]
fn malformed_cache_miss_response_wire_remains_a_fatal_decode_error() {
    let truncated = raw_packet(PacketId::ClientCacheMissResponse, &[0x01]);

    assert!(
        super::decode_test_raw(truncated).is_err(),
        "a declared blob without its hash and payload must fail closed in raw decode"
    );
}

#[test]
fn ignored_play_packet_is_not_materialized() {
    let raw = raw_packet(PacketId::NetworkSettings, &[0x7f]);
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
    })
    .expect("ignored packet");

    assert!(event.is_none());
    assert!(!decoder_called.get());
}

#[test]
fn allowlisted_ui_packet_is_validated_decoded_and_normalized() {
    let packet: Packet = crate::Packet::new(Text {
        body: TextData::MessageOnly {
            message_type: TextPacketType::Raw,
            message: "live UI".into(),
        },
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode text");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw text");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
    })
    .expect("decode UI event");

    assert!(decoder_called.get());
    assert!(matches!(
        event,
        Some(WorldEvent::Ui(crate::UiEvent::Text(_)))
    ));
}

#[test]
fn live_ui_path_rejects_invalid_utf8_as_a_semantic_skip() {
    let raw = raw_packet(PacketId::Text, &[0, 0, 0, 1, 0xff, 0, 0, 0]);

    let error =
        decode_world_raw_with(raw, 0, super::decode_test_raw).expect_err("invalid UI UTF-8");

    assert!(
        matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Ui(
                crate::UiPacketError::InvalidUtf8 { .. }
            ))
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn live_score_path_bounds_text_as_a_semantic_skip() {
    // One `changeplayer` entry whose objective name is over the text bound: entry count, then
    // per entry a variant tag, the lowercase variant name, the entry ID, and the variant body.
    let mut body = Vec::new();
    wire::Writer::new(&mut body).write_var_u32(1).unwrap();
    body.put_u8(1);
    wire::Writer::new(&mut body)
        .write_var_u32("changeplayer".len() as u32)
        .unwrap();
    body.put_slice(b"changeplayer");
    wire::Writer::new(&mut body).write_var_u64(0).unwrap();
    wire::Writer::new(&mut body)
        .write_var_u32((crate::MAX_UI_TEXT_BYTES + 1) as u32)
        .unwrap();
    body.put_bytes(b'x', crate::MAX_UI_TEXT_BYTES + 1);
    let raw = raw_packet(PacketId::SetScore, &body);

    let error =
        decode_world_raw_with(raw, 0, super::decode_test_raw).expect_err("oversized score text");

    assert!(
        matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Ui(
                crate::UiPacketError::TextTooLong { .. }
            ))
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn live_ui_semantic_rejection_is_skippable_world_error() {
    // A well-formed Text packet whose category selector is outside the known set is
    // semantically odd, not malformed wire: it must stay a skippable world rejection.
    // Body: needs_translation=false (0x00), category=3 (unknown, > 2).
    let raw = raw_packet(PacketId::Text, &[0x00, 0x03]);

    let error =
        decode_world_raw_with(raw, 0, super::decode_test_raw).expect_err("unknown text category");

    assert!(
        matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Ui(
                crate::UiPacketError::UnknownEnum { value: 3, .. }
            ))
        ),
        "unexpected error: {error:?}"
    );
}

#[test]
fn live_inventory_content_checks_slot_count_before_owned_decoder() {
    let mut body = Vec::new();
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    wire::Writer::new(&mut body)
        .write_var_u32((crate::MAX_CONTAINER_SLOTS + 1) as u32)
        .unwrap();
    let raw = raw_packet(PacketId::InventoryContent, &body);
    let decoder_called = Cell::new(false);

    let error = decode_world_raw_with(raw, 0, |_| {
        decoder_called.set(true);
        panic!("oversized inventory content must fail before owned decoding")
    })
    .expect_err("oversized inventory content");

    assert!(!decoder_called.get());
    assert!(matches!(
        error,
        ProtocolError::World(crate::WorldPacketError::Wire(
            crate::WorldWireError::Inventory(crate::InventoryPacketError::MalformedWire)
        ))
    ));
}

#[test]
fn live_stack_response_checks_nested_counts_before_owned_decoder() {
    let cases = [
        (
            raw_packet(
                PacketId::ItemStackResponse,
                &varint_body((crate::MAX_STACK_RESPONSES + 1) as u32),
            ),
            "responses",
        ),
        (
            raw_packet(
                PacketId::ItemStackResponse,
                &accepted_response_prefix((crate::MAX_RESPONSE_CONTAINERS + 1) as u32),
            ),
            "containers",
        ),
        (
            raw_packet(
                PacketId::ItemStackResponse,
                &accepted_response_with_slot_count((crate::MAX_CONTAINER_SLOTS + 1) as u32),
            ),
            "slots",
        ),
        (
            raw_packet(
                PacketId::ItemStackResponse,
                &accepted_response_with_name_lengths(
                    (crate::MAX_RESPONSE_NAME_BYTES + 1) as u32,
                    0,
                ),
            ),
            "unredacted name",
        ),
        (
            raw_packet(
                PacketId::ItemStackResponse,
                &accepted_response_with_name_lengths(
                    0,
                    (crate::MAX_RESPONSE_NAME_BYTES + 1) as u32,
                ),
            ),
            "redacted name",
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
                ProtocolError::World(crate::WorldPacketError::Wire(
                    crate::WorldWireError::Inventory(crate::InventoryPacketError::MalformedWire)
                ))
            ),
            "unexpected {label} error: {error:?}"
        );
    }
}

#[test]
fn live_inventory_items_check_extra_length_before_owned_decoder() {
    let mut content = Vec::new();
    wire::Writer::new(&mut content).write_var_u32(0).unwrap();
    wire::Writer::new(&mut content).write_var_u32(1).unwrap();
    append_item_v4_prefix(&mut content, (crate::MAX_ITEM_EXTRA_BYTES + 1) as u32);

    let mut slot = Vec::new();
    wire::Writer::new(&mut slot).write_var_u32(0).unwrap();
    wire::Writer::new(&mut slot).write_var_u32(0).unwrap();
    slot.put_u8(0);
    slot.put_u8(0);
    append_item_new_prefix(&mut slot, (crate::MAX_ITEM_EXTRA_BYTES + 1) as u32);

    for (id, body) in [
        (PacketId::InventoryContent, content),
        (PacketId::InventorySlot, slot),
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
            ProtocolError::World(crate::WorldPacketError::Wire(
                crate::WorldWireError::Inventory(crate::InventoryPacketError::MalformedWire)
            ))
        ));
    }
}

#[test]
fn canonical_inventory_fixtures_pass_raw_gate_and_owned_normalization() {
    for (name, fixture) in [
        (
            "inventory_content",
            &include_bytes!("../../fixtures/inventory_content.bin")[..],
        ),
        (
            "inventory_slot",
            &include_bytes!("../../fixtures/inventory_slot.bin")[..],
        ),
        (
            "player_hotbar",
            &include_bytes!("../../fixtures/player_hotbar.bin")[..],
        ),
        (
            "item_stack_response",
            &include_bytes!("../../fixtures/item_stack_response.bin")[..],
        ),
    ] {
        let mut batch = Bytes::copy_from_slice(fixture);
        assert_eq!(batch.get_u8(), 0xfe);
        let raw = decode_packet_raw(&mut batch).expect("raw inventory fixture");
        let event = decode_world_raw_with(raw, 0, super::decode_test_raw)
            .unwrap_or_else(|error| panic!("canonical {name} fixture: {error}"))
            .expect("inventory world event");
        assert!(matches!(event, WorldEvent::Inventory(_)));
    }
}

fn varint_body(value: u32) -> Vec<u8> {
    let mut body = Vec::new();
    wire::Writer::new(&mut body).write_var_u32(value).unwrap();
    body
}

fn accepted_response_prefix(container_count: u32) -> Vec<u8> {
    let mut body = Vec::new();
    wire::Writer::new(&mut body).write_var_u32(1).unwrap();
    body.put_u8(0);
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    body.put_u8(1);
    wire::Writer::new(&mut body)
        .write_var_u32(container_count)
        .unwrap();
    body
}

fn accepted_response_with_slot_count(slot_count: u32) -> Vec<u8> {
    let mut body = accepted_response_prefix(1);
    body.put_u8(0);
    body.put_u8(0);
    wire::Writer::new(&mut body)
        .write_var_u32(slot_count)
        .unwrap();
    body
}

fn accepted_response_with_name_lengths(unredacted: u32, redacted: u32) -> Vec<u8> {
    let mut body = accepted_response_with_slot_count(1);
    body.extend_from_slice(&[0, 0, 0]);
    body.put_u8(0);
    wire::Writer::new(&mut body)
        .write_var_u32(unredacted)
        .unwrap();
    if unredacted <= crate::MAX_RESPONSE_NAME_BYTES as u32 {
        body.put_u8(1);
        wire::Writer::new(&mut body)
            .write_var_u32(redacted)
            .unwrap();
    }
    body
}

fn append_item_v4_prefix(body: &mut Vec<u8>, extra_length: u32) {
    body.put_i16_le(1);
    body.put_u16_le(1);
    wire::Writer::new(body).write_var_u32(0).unwrap();
    body.put_u8(0);
    wire::Writer::new(body).write_var_u32(0).unwrap();
    wire::Writer::new(body).write_var_u32(extra_length).unwrap();
}

fn append_item_new_prefix(body: &mut Vec<u8>, extra_length: u32) {
    body.put_i16_le(1);
    body.put_u16_le(1);
    wire::Writer::new(body).write_var_u32(0).unwrap();
    body.put_u8(0);
    wire::Writer::new(body).write_var_u32(0).unwrap();
    wire::Writer::new(body).write_var_u32(extra_length).unwrap();
}

#[test]
fn death_information_reaches_world_ui_from_raw_transport() {
    let packet: Packet = crate::Packet::new(DeathInfo {
        death_cause_attack_name: "death.attack.mob".into(),
        death_cause_message_list: vec!["Alex".into(), "%entity.zombie.name".into()],
    });
    let mut batch = crate::encode(&packet).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    let event = decode_world_raw_with(raw, 0, super::decode_test_raw).unwrap();
    assert_eq!(
        event,
        Some(WorldEvent::Ui(crate::UiEvent::DeathInfo(
            crate::DeathInfoEvent {
                message: "death.attack.mob".into(),
                parameters: vec![Arc::from("Alex"), Arc::from("%entity.zombie.name")].into(),
            }
        )))
    );
}

#[test]
fn allowlisted_world_packet_is_decoded_and_normalized() {
    let packet: Packet = crate::Packet::new(UpdateBlock {
        block_position: block_pos(17, 2, -3),
        block_runtime_id: wire::VarUInt(99),
        layer: wire::VarUInt(0),
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode update");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw update");

    let event = decode_world_raw_with(raw, 2, super::decode_test_raw)
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
    let nbt = vec![10, 0, 0];
    let packet: Packet = crate::Packet::new(BlockActorData {
        block_position: block_pos(17, 2, -3),
        actor_data_tags: wire::NetworkNbt(Bytes::copy_from_slice(&nbt)),
    });
    let mut batch = crate::encode(&packet).expect("encode block entity update");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw block entity update");

    let event = decode_world_raw_with(raw, 2, super::decode_test_raw)
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
    let packet: Packet = crate::Packet::new(LevelEvent {
        // LevelEventStartRaining; the packet carries the raw id, not an enum.
        event_id: wire::ZigZag32(3001),
        data: wire::ZigZag32(48_000),
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode weather event");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw weather event");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
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
    let packet: Packet = crate::Packet::new(GameRulesChanged {
        rule_data: GameRulesChangedData {
            rules_list: vec![GameRule {
                rule_name: "DoDaylightCycle".into(),
                rule_can_be_modified: false,
                rule_value: GameRuleValue::Bool(false),
            }],
        },
    });
    let mut batch = crate::encode(&packet).expect("encode gamerule event");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw gamerule event");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
    })
    .expect("decode gamerule event");

    assert!(decoder_called.get());
    let Some(WorldEvent::GameRules(rules)) = event else {
        panic!("{event:?}")
    };
    assert_eq!(rules.daylight_cycle.map(|cycle| cycle.enabled), Some(false));
}

#[test]
fn death_screen_rules_reach_the_raw_world_stream() {
    let packet: Packet = crate::Packet::new(GameRulesChanged {
        rule_data: GameRulesChangedData {
            rules_list: vec![GameRule {
                rule_name: "showDeathMessages".into(),
                rule_can_be_modified: false,
                rule_value: GameRuleValue::Bool(false),
            }],
        },
    });
    let mut batch = crate::encode(&packet).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    let event = decode_world_raw_with(raw, 0, super::decode_test_raw).unwrap();
    let Some(WorldEvent::GameRules(rules)) = event else {
        panic!("missing death rules: {event:?}");
    };
    assert_eq!(rules.death.show_messages, Some(false));
    assert_eq!(rules.death.immediate_respawn, None);
}

#[test]
fn allowlisted_move_player_is_materialized_and_normalized() {
    let packet: Packet = crate::Packet::new(MovePlayer {
        player_runtime_id: ActorRuntimeID(42),
        position: [1.25, 70.5, -8.75].into(),
        rotation: [15.0, -120.25].into(),
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode move player");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw move player");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
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
            riding_runtime_id: 0,
            teleport_cause: None,
        })
    );
}

#[test]
fn allowlisted_respawn_is_materialized_and_normalized() {
    let packet: Packet = crate::Packet::new(Respawn {
        position: [8.5, 71.620_01, -4.25].into(),
        state: PlayerRespawnState::Readytospawn,
        player_runtime_id: ActorRuntimeID(42),
    });
    let mut batch = crate::encode(&packet).expect("encode respawn");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw respawn");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
    })
    .expect("decode respawn")
    .expect("respawn event");

    assert!(decoder_called.get());
    assert_eq!(
        event,
        WorldEvent::Respawn(crate::RespawnEvent {
            position: [8.5, 71.620_01, -4.25],
            state: 1,
            runtime_entity_id: 42,
        })
    );
}

#[test]
fn allowlisted_actor_packet_is_materialized_and_normalized() {
    let packet: Packet = crate::Packet::new(AddActor {
        target_actor_id: ActorUniqueID(9),
        target_runtime_id: ActorRuntimeID(42),
        actor_type: "minecraft:bee".into(),
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode add entity");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw add entity");

    let event = decode_world_raw_with(raw, 2, super::decode_test_raw).expect("decode actor event");

    assert!(matches!(
        event,
        Some(WorldEvent::Actor(crate::ActorEvent::Spawn(spawn)))
            if spawn.dimension == 2 && spawn.runtime_id == 42
    ));
}

#[test]
fn painting_wire_packet_reaches_play_dispatch_with_its_motive() {
    let packet = crate::Packet::new(bedrock_protocol_1_26_51::packets::AddPainting {
        target_actor_id: ActorUniqueID(-15),
        target_runtime_id: ActorRuntimeID(73),
        position: [3.5, 70.5, 10.03125].into(),
        direction: wire::ZigZag32(2),
        motif: "backyard".into(),
    });
    let mut batch = crate::encode(&packet).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    let event = decode_world_raw_with(raw, 0, super::decode_test_raw).unwrap();
    assert!(
        matches!(event,Some(WorldEvent::Actor(crate::ActorEvent::PaintingSpawn {spawn,painting}))
        if spawn.runtime_id==73 && spawn.unique_id == -15 && painting.motive.as_ref()=="backyard")
    );
}

#[test]
fn allowlisted_item_and_action_packets_are_materialized_and_normalized() {
    let packets: [Packet; 4] = [
        crate::Packet::new(AddPlayer {
            target_runtime_id: ActorRuntimeID(42),
            ..Default::default()
        }),
        crate::Packet::new(MobEquipment {
            target_runtime_id: ActorRuntimeID(42),
            item: NetworkItemStackDescriptorSerializedData {
                id: wire::I16LE(5),
                stack_size: wire::U16LE(1),
                ..Default::default()
            },
            slot: wire::U8(0),
            selected_slot: wire::U8(0),
            // The inventory container ID.
            container_id: wire::U8(0),
        }),
        crate::Packet::new(Animate {
            target_actor_runtime_id: ActorRuntimeID(42),
            action: AnimateAction::Swing,
            ..Default::default()
        }),
        crate::Packet::new(AnimateEntity {
            m_animation: "animation.test.attack".into(),
            m_controller: "controller.animation.test".into(),
            m_runtime_ids: vec![ActorRuntimeID(42)],
            ..Default::default()
        }),
    ];

    for packet in packets {
        let mut batch = crate::encode(&packet).expect("encode item/action packet");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw item/action packet");
        let event = decode_world_raw_with(raw, 0, super::decode_test_raw)
            .expect("decode item/action event");
        assert!(event.is_some());
    }
}

#[test]
fn canonical_empty_mob_equipment_is_materialized_and_normalized() {
    let mut body = Vec::new();
    wire::Writer::new(&mut body).write_var_u64(42).unwrap();
    body.put_i16_le(0);
    body.put_u16_le(0);
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    body.put_u8(0);
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    body.put_u8(0);
    body.put_u8(0);
    body.put_i8(0);
    let raw = raw_packet(PacketId::MobEquipment, &body);

    let event = decode_world_raw_with(raw, 0, super::decode_test_raw)
        .expect("decode empty equipment")
        .expect("empty equipment event");

    assert!(matches!(
        event,
        WorldEvent::Equipment(crate::EquipmentEvent { stack, .. }) if stack == crate::NetworkItemStack::empty()
    ));
}

/// Builds raw non-air equipment with controlled count, extra data, and container ID.
fn raw_mob_equipment(count: u16, extra: &[u8], container_id: i8) -> RawPacket {
    let mut body = Vec::new();
    wire::Writer::new(&mut body).write_var_u64(42).unwrap();
    body.put_i16_le(5);
    body.put_u16_le(count);
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    body.put_u8(0);
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    wire::Writer::new(&mut body)
        .write_var_u32(extra.len() as u32)
        .unwrap();
    body.put_slice(extra);
    body.put_u8(0);
    body.put_u8(0);
    body.put_i8(container_id);
    raw_packet(PacketId::MobEquipment, &body)
}

#[test]
fn valid_equipment_is_retained_and_invalid_items_are_rejected() {
    // Byte-exact wire canonicalization was intentionally removed: the client
    // only renders retained items, so faithful round-tripping is unnecessary
    // and rejected legitimate servers. Semantic validation is what guards
    // retention now.
    let valid_extra = [0; 10];
    let valid = decode_world_raw_with(raw_mob_equipment(1, &valid_extra, 0), 0, |raw| {
        super::decode_test_raw(raw)
    })
    .expect("valid equipment wire");
    assert!(matches!(valid, Some(WorldEvent::Equipment(_))));

    let error = decode_world_raw_with(raw_mob_equipment(0, &valid_extra, 0), 0, |raw| {
        super::decode_test_raw(raw)
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

    let error = decode_world_raw_with(raw_mob_equipment(1, &valid_extra, -1), 0, |raw| {
        super::decode_test_raw(raw)
    })
    .expect_err("unknown equipment container must be rejected semantically");
    assert!(matches!(
        &error,
        ProtocolError::World(crate::WorldPacketError::Item(
            crate::ItemPacketError::UnknownEquipmentContainer(u8::MAX)
        ))
    ));

    let mut world_skips = 0;
    skip_semantic_world_error(error, &mut world_skips)
        .expect("semantic equipment rejection must keep the session alive");
    assert_eq!(world_skips, 1);
}

#[test]
fn absolute_actor_move_uses_bedrock_varuint_and_raw_byte_rotations() {
    let runtime_id = (u64::from(u32::MAX) + 42) << 1;
    let mut body = Vec::new();
    wire::Writer::new(&mut body)
        .write_var_u64(runtime_id)
        .unwrap();
    body.put_u8(0b11);
    body.put_f32_le(12.5);
    body.put_f32_le(64.25);
    body.put_f32_le(-7.75);
    body.put_u8(32);
    body.put_u8(64);
    body.put_u8(128);
    let raw = raw_packet(PacketId::MoveActorAbsolute, &body);
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 2, |_| {
        decoder_called.set(true);
        panic!("absolute actor movement uses the raw decoder")
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
            head_yaw: Some(-180.0),
            on_ground: Some(true),
            teleported: true,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }))
    );
}

#[test]
fn absolute_actor_move_rejects_truncated_and_trailing_bodies() {
    let mut valid = Vec::new();
    wire::Writer::new(&mut valid).write_var_u64(42).unwrap();
    valid.put_u8(0);
    valid.put_f32_le(1.0);
    valid.put_f32_le(2.0);
    valid.put_f32_le(3.0);
    valid.put_slice(&[0, 0, 0]);

    for (body, actual) in [
        (&valid[..valid.len() - 1], 15_usize),
        (&[&valid[..], &[0xff][..]].concat(), 17_usize),
    ] {
        let raw = raw_packet(PacketId::MoveActorAbsolute, body);
        let error = decode_world_raw_with(raw, 0, |_| {
            panic!("malformed absolute actor movement uses the raw decoder")
        })
        .expect_err("malformed absolute actor move");
        assert!(matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Wire(
                crate::WorldWireError::Actor(
                    crate::ActorPacketError::InvalidAbsoluteMoveLength {
                        actual: found,
                        expected: 16,
                    }
                )
            )) if found == actual
        ));
    }
}

#[test]
fn allowlisted_movement_correction_is_materialized_and_normalized() {
    let packet: Packet = crate::Packet::new(CorrectPlayerMovePrediction {
        pos: [27.5, 111.0, 91.5].into(),
        pos_delta: [0.5, -0.25, 1.0].into(),
        rotation: [-15.0, 90.25].into(),
        on_ground: true,
        tick: PlayerInputTick(55),
        ..Default::default()
    });
    let mut batch = crate::encode(&packet).expect("encode movement correction");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw movement correction");
    let decoder_called = Cell::new(false);
    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
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
            subject: crate::MovementCorrectionSubject::Player,
            on_ground: true,
            tick: 55,
            vehicle_angular_velocity: None,
        })
    );
}

#[test]
fn allowlisted_biome_definitions_are_materialized_and_normalized() {
    let packet: Packet = crate::Packet::new(BiomeDefinitionList {
        // The key indexes into the string list; the id is the biome's own.
        map_of_biome_names_to_data: vec![(
            wire::U16LE(0),
            BiomeDefinitionData {
                id: wire::U16LE(u16::MAX),
                temperature: wire::F32LE(0.8),
                downfall: wire::F32LE(0.4),
                foliage_snow: wire::F32LE(0.0),
                map_water_color_argb: wire::I32LE(0xff44_6688_u32 as i32),
                ..Default::default()
            },
        )],
        string_list: BiomeStringList {
            strings: vec!["plains".into()],
        },
    });
    let mut batch = crate::encode(&packet).expect("encode biome definitions");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw biome definitions");
    let decoder_called = Cell::new(false);

    let event = decode_world_raw_with(raw, 0, |raw| {
        decoder_called.set(true);
        super::decode_test_raw(raw)
    })
    .expect("decode biome definitions");

    assert!(decoder_called.get());
    let WorldEvent::BiomeDefinitions(event) = event.expect("biome definitions event") else {
        panic!("expected biome definitions")
    };
    assert_eq!(event.definitions[0].biome_id, None);
    assert_eq!(event.definitions[0].name.as_ref(), "minecraft:plains");
}

/// A block position in the generated wire shape.
fn block_pos(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos {
        x: wire::ZigZag32(x),
        y: wire::ZigZag32(y),
        z: wire::ZigZag32(z),
    }
}

/// A cached SubChunk keeps only its own entry alive, not the batch it arrived in.
#[test]
fn cached_sub_chunks_detach_from_their_batch() {
    use bedrock_protocol_1_26_51::enums::SubChunkRequestResult;
    use bedrock_protocol_1_26_51::packets::{ScriptMessage, SubChunk};
    use bedrock_protocol_1_26_51::types::SubChunkData;

    let payload = vec![0x5a; 64];
    let sub_chunk = Packet::new(SubChunk {
        cache_enabled: true,
        sub_chunk_data: vec![SubChunkData {
            sub_chunk_request_result: SubChunkRequestResult::Success,
            serialized_sub_chunk: Some(payload.clone().into()),
            blob_id: Some(wire::U64LE(7)),
            ..Default::default()
        }],
        ..Default::default()
    });
    let sibling = Packet::new(ScriptMessage {
        message_id: "sibling".into(),
        message_value: vec![0; 1 << 20].into(),
    });
    let mut batch = Vec::new();
    crate::codec::encode_entry(&sub_chunk, &mut batch).unwrap();
    crate::codec::encode_entry(&sibling, &mut batch).unwrap();
    let batch = Bytes::from(batch);

    let raw = crate::raw_packet::decode_batch_body(batch.clone(), usize::MAX)
        .unwrap()
        .remove(0);
    let entry_len = raw.inner_frame().len();
    let raw = detach_cached_chunk(raw);
    let frame = raw.inner_frame().clone();
    assert_eq!(frame.len(), entry_len);
    assert!(
        !batch.as_ptr_range().contains(&frame.as_ptr()),
        "the entry no longer shares the batch allocation"
    );
    let packets::Packet::SubChunk(decoded) = *raw.decode().unwrap().data else {
        panic!("a SubChunk");
    };
    let retained = decoded.sub_chunk_data[0]
        .serialized_sub_chunk
        .clone()
        .expect("a payload");
    assert_eq!(&retained[..], &payload[..]);
    assert!(
        frame.as_ptr_range().contains(&retained.as_ptr()),
        "the retained payload slices only its detached entry"
    );
}
