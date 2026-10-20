//! Opt-in witness from the same plaintext batch at its original successful write boundary.
use bedrock_protocol_1_26_51::packets;
use bytes::Bytes;

/// Returns only the last finite own movement anchor decoded by the existing packet owner.
/// Malformed or unrelated entries cannot change the original bytes or fail a write.
pub(super) fn own_position(body: &Bytes, actor: u64) -> Option<[f32; 3]> {
    let entries =
        crate::raw_packet::decode_batch_body(body.clone(), super::super::MAX_SESSION_BATCH_BYTES)
            .ok()?;
    let mut position = None;
    for entry in entries {
        if !matches!(
            entry.id,
            packets::PacketId::PlayerAuthInput | packets::PacketId::MovePlayer
        ) {
            continue;
        }
        let Ok((_, packet)) = entry.decode_parts() else {
            continue;
        };
        let candidate = match packet {
            packets::Packet::PlayerAuthInput(packet) => Some(packet.position.to_array()),
            packets::Packet::MovePlayer(packet)
                if actor != 0
                    && packet.player_runtime_id.0 == actor
                    && crate::MovePlayerMode::from(packet.position_mode)
                        != crate::MovePlayerMode::Rotation =>
            {
                Some(packet.position.to_array())
            }
            _ => None,
        };
        if let Some(candidate) = candidate
            && candidate.iter().all(|value| value.is_finite())
        {
            position = Some(candidate);
        }
    }
    position
}
