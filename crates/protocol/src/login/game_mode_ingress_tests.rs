//! Real raw ingress for BDS's targeted game-mode confirmation packet.

use bytes::BytesMut;
use valentine::bedrock::version::v1_26_51::*;

fn raw_packet(
    mode: EnumsGameType,
    actor_unique_id: i64,
    tick: u64,
) -> crate::raw_packet::RawPacket {
    let packet: crate::Packet = crate::Packet::new(UpdatePlayerGameTypePacket {
        player_game_type: mode,
        targetplayer: ActorUniqueId { actor_unique_id },
        tick: PlayerInputTick { inputtick: tick },
    });
    let mut frame = BytesMut::new();
    packet
        .data
        .encode_inner_bytes_mut(&mut frame, 0, 0)
        .unwrap();
    crate::raw_packet::decode_packet_raw(&mut frame.freeze()).unwrap()
}

#[test]
fn update_player_game_type_enters_raw_ingress_with_unique_id_and_tick() {
    for (wire, expected) in [
        (
            EnumsGameType::Survival,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Survival),
        ),
        (
            EnumsGameType::Creative,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Creative),
        ),
        (
            EnumsGameType::Adventure,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Adventure),
        ),
        (
            EnumsGameType::Spectator,
            crate::GameModeUpdate::Explicit(crate::PlayerGameMode::Spectator),
        ),
        (EnumsGameType::Default, crate::GameModeUpdate::WorldDefault),
        (
            EnumsGameType::Unknown(77),
            crate::GameModeUpdate::Unknown(77),
        ),
    ] {
        let raw = raw_packet(wire, -55, u64::MAX);
        assert_eq!(raw.id, McpePacketName::UpdatePlayerGameTypePacket);
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
    let packet = raw_packet(EnumsGameType::Creative, 42, 1);
    let mut payload = BytesMut::new();
    valentine::protocol::wire::write_var_u32(
        &mut payload,
        McpePacketName::UpdatePlayerGameTypePacket as u32,
    );
    payload.extend_from_slice(&packet.body()[..packet.body().len() - 1]);
    let mut frame = BytesMut::new();
    valentine::protocol::wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    let raw = crate::raw_packet::decode_packet_raw(&mut frame.freeze()).unwrap();
    let result = super::decode_world_raw_with(raw, 0, super::decode_test_raw);
    assert!(result.is_err());
    let mut skipped = 0;
    assert!(super::skip_semantic_world_error(result.unwrap_err(), &mut skipped).is_err());
    assert_eq!(skipped, 0);
}
