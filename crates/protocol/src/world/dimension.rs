use valentine::bedrock::version::v1_26_51::{
    ChangeDimensionPacket, EnumsPlayerRespawnState, RespawnPacket,
};

use super::{ChangeDimensionEvent, RespawnEvent};

pub(super) fn normalize_change_dimension(packet: &ChangeDimensionPacket) -> ChangeDimensionEvent {
    ChangeDimensionEvent {
        dimension: packet.dimension_id.value,
        position: [packet.position.x, packet.position.y, packet.position.z],
        respawn: packet.respawn,
        loading_screen_id: packet.loading_screen_id,
    }
}

pub(super) fn normalize_respawn(packet: &RespawnPacket) -> RespawnEvent {
    RespawnEvent {
        position: [packet.position.x, packet.position.y, packet.position.z],
        state: match packet.state {
            EnumsPlayerRespawnState::Searchingforspawn => 0,
            EnumsPlayerRespawnState::Readytospawn => 1,
            EnumsPlayerRespawnState::Clientreadytospawn => 2,
            EnumsPlayerRespawnState::Unknown(value) => value,
        },
        runtime_entity_id: packet.player_runtime_id.actor_runtime_id,
    }
}
