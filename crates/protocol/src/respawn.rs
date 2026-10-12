//! Native death-screen request and ready-to-spawn completion.

use bedrock_protocol_1_26_51::enums::{PlayerActionType, PlayerRespawnState};
use bedrock_protocol_1_26_51::packets::{PlayerAction, Respawn};
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bedrock_protocol_1_26_51::wire;

/// Requests respawn readiness with a zero position and the local player's runtime ID.
#[must_use]
pub fn respawn_request_packet(local_runtime_id: u64) -> crate::Packet {
    crate::Packet::new(Respawn {
        state: PlayerRespawnState::Clientreadytospawn,
        player_runtime_id: ActorRuntimeID(local_runtime_id),
        ..Default::default()
    })
}

/// Completes respawn readiness with zero block positions, face -1, and the local runtime ID.
#[must_use]
pub fn respawn_ready_packet(local_runtime_id: u64) -> crate::Packet {
    crate::Packet::new(PlayerAction {
        action: PlayerActionType::Respawn,
        face: wire::ZigZag32(-1),
        player_runtime_id: ActorRuntimeID(local_runtime_id),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock_protocol_1_26_51::packets;
    use bedrock_protocol_1_26_51::types::BlockPos;

    #[test]
    fn request_and_completion_use_the_local_actor_with_distinct_native_states() {
        let packets::Packet::Respawn(request) = *respawn_request_packet(77).data else {
            panic!("respawn request");
        };
        assert_eq!(request.state, PlayerRespawnState::Clientreadytospawn);
        assert_eq!(request.position.to_array(), [0.0; 3]);
        assert_eq!(request.player_runtime_id.0, 77);
        let packets::Packet::PlayerAction(ready) = *respawn_ready_packet(77).data else {
            panic!("respawn completion");
        };
        assert_eq!(ready.action, PlayerActionType::Respawn);
        assert_eq!(ready.face.0, -1);
        assert_eq!(ready.block_position, BlockPos::default());
        assert_eq!(ready.result_pos, BlockPos::default());
        assert_eq!(ready.player_runtime_id.0, 77);
    }
}
