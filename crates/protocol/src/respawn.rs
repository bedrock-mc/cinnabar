//! Native death-screen request and ready-to-spawn completion.

use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, EnumsPlayerActionType, EnumsPlayerRespawnState, PlayerActionPacket,
    RespawnPacket,
};

/// Requests respawn readiness with a zero position and the local player's runtime ID.
#[must_use]
pub fn respawn_request_packet(local_runtime_id: u64) -> crate::Packet {
    RespawnPacket {
        state: EnumsPlayerRespawnState::Clientreadytospawn,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: local_runtime_id,
        },
        ..Default::default()
    }
    .into()
}

/// Completes respawn readiness with zero block positions, face -1, and the local runtime ID.
#[must_use]
pub fn respawn_ready_packet(local_runtime_id: u64) -> crate::Packet {
    PlayerActionPacket {
        action: EnumsPlayerActionType::Respawn,
        face: -1,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: local_runtime_id,
        },
        ..Default::default()
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use valentine::bedrock::version::v1_26_51::{BlockPos, McpePacketData, Vec3};

    #[test]
    fn request_and_completion_use_the_local_actor_with_distinct_native_states() {
        let McpePacketData::RespawnPacket(request) = respawn_request_packet(77).data else {
            panic!("respawn request");
        };
        assert_eq!(request.state, EnumsPlayerRespawnState::Clientreadytospawn);
        assert_eq!(request.position, Vec3::default());
        assert_eq!(request.player_runtime_id.actor_runtime_id, 77);
        let McpePacketData::PlayerActionPacket(ready) = respawn_ready_packet(77).data else {
            panic!("respawn completion");
        };
        assert_eq!(ready.action, EnumsPlayerActionType::Respawn);
        assert_eq!(ready.face, -1);
        assert_eq!(ready.block_position, BlockPos::default());
        assert_eq!(ready.result_pos, BlockPos::default());
        assert_eq!(ready.player_runtime_id.actor_runtime_id, 77);
    }
}
