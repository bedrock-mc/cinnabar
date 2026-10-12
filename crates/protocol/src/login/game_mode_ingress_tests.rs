//! Real raw ingress for BDS's targeted game-mode confirmation packet.

use bedrock_protocol_1_26_51::enums::GameType;
use bedrock_protocol_1_26_51::packets::{PacketId, UpdatePlayerGameType};
use bedrock_protocol_1_26_51::types::{ActorUniqueID, PlayerInputTick};
use bedrock_protocol_1_26_51::wire;

fn raw_packet(mode: GameType, actor_unique_id: i64, tick: u64) -> crate::raw_packet::RawPacket {
    let packet: crate::Packet = crate::Packet::new(UpdatePlayerGameType {
        player_game_type: mode,
        target_player: ActorUniqueID(actor_unique_id),
        tick: PlayerInputTick(tick),
    });
    let mut frame = Vec::new();
    crate::codec::encode_entry(&packet, &mut frame).unwrap();
    crate::raw_packet::decode_packet_raw(&mut bytes::Bytes::from(frame)).unwrap()
}

#[test]
fn update_player_game_type_enters_raw_ingress_with_unique_id_and_tick() {
    for (wire, expected) in [
        (
            GameType::Survival,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Survival),
        ),
        (
            GameType::Creative,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Creative),
        ),
        (
            GameType::Adventure,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Adventure),
        ),
        (
            GameType::Spectator,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Spectator),
        ),
        (GameType::Default, crate::GameModeUpdate::WorldDefault),
        (GameType::Unknown(77), crate::GameModeUpdate::Unknown(77)),
    ] {
        let raw = raw_packet(wire, -55, u64::MAX);
        assert_eq!(raw.id, PacketId::UpdatePlayerGameType);
        let event = super::decode_world_raw_with(raw, 0, super::decode_test_raw).unwrap();
        assert_eq!(
            event,
            Some(crate::WorldEvent::Ui(crate::UiEvent::PlayerGameMode {
                actor_unique_id: -55,
                tick: u64::MAX,
                event: crate::GameModeEvent { update: expected },
            }))
        );
    }
}

#[test]
fn truncated_targeted_game_mode_is_a_fatal_wire_fault_not_an_ignored_packet() {
    let packet = raw_packet(GameType::Creative, 42, 1);
    let mut payload = Vec::new();
    wire::Writer::new(&mut payload)
        .write_var_u32(PacketId::UpdatePlayerGameType as u32)
        .unwrap();
    payload.extend_from_slice(&packet.body()[..packet.body().len() - 1]);
    let mut frame = Vec::new();
    wire::Writer::new(&mut frame)
        .write_var_u32(payload.len() as u32)
        .unwrap();
    frame.extend_from_slice(&payload);
    let raw = crate::raw_packet::decode_packet_raw(&mut bytes::Bytes::from(frame)).unwrap();
    let result = super::decode_world_raw_with(raw, 0, super::decode_test_raw);
    assert!(result.is_err());
    let mut skipped = 0;
    assert!(super::skip_semantic_world_error(result.unwrap_err(), &mut skipped).is_err());
    assert_eq!(skipped, 0);
}
