use bedrock_protocol_1_26_51::packets::{ChangeDimension, Respawn};

use super::{ChangeDimensionEvent, RespawnEvent};

pub(super) fn normalize_change_dimension(packet: &ChangeDimension) -> ChangeDimensionEvent {
    ChangeDimensionEvent {
        dimension: packet.dimension_id.0,
        position: packet.position.to_array(),
        respawn: packet.respawn,
        loading_screen_id: packet.loading_screen_id.map(|id| id.0),
    }
}

pub(super) fn normalize_respawn(packet: &Respawn) -> RespawnEvent {
    RespawnEvent {
        position: packet.position.to_array(),
        state: packet.state.to_raw(),
        runtime_entity_id: packet.player_runtime_id.0,
    }
}
