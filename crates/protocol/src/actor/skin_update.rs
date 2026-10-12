use bedrock_protocol_1_26_51::packets::PlayerSkin as PlayerSkinPacket;

use super::{ActorEvent, normalize_player_skin};

/// Retains an appearance change without inventing a player-list entry.
pub(crate) fn normalize_skin_update(packet: PlayerSkinPacket) -> ActorEvent {
    ActorEvent::Skin {
        uuid: *packet.uuid.as_bytes(),
        skin: normalize_player_skin(packet.serialized_skin, &mut 0),
    }
}
