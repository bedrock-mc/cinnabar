//! Outbound packets for block-side requests: sign edits and map image requests.

use bytes::Bytes;
use valentine::bedrock::{
    codec::Nbt,
    version::v1_26_51::{BlockActorDataPacket, BlockPos, MapInfoRequestPacket},
};

use crate::Packet;

/// The `BlockActorData` a client sends when it closes a sign editor; `nbt` is the whole sign
/// compound as NetworkLittleEndian bytes.
#[must_use]
pub fn sign_edit_packet(position: [i32; 3], nbt: &[u8]) -> Packet {
    crate::Packet::new(BlockActorDataPacket {
        block_position: BlockPos {
            x: position[0],
            y: position[1],
            z: position[2],
        },
        actor_data_tags: Nbt(Bytes::copy_from_slice(nbt)),
    })
}

/// Asks the server for the pixels of `map_id`, as a client does on first sight of a map.
#[must_use]
pub fn map_info_request_packet(map_id: i64) -> Packet {
    crate::Packet::new(MapInfoRequestPacket {
        map_unique_id: valentine::bedrock::version::v1_26_51::ActorUniqueId {
            actor_unique_id: map_id,
        },
        client_pixels_list: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_wrap_the_expected_wire_packets() {
        let sign = sign_edit_packet([1, 2, 3], &[10, 0, 0]);
        let map = map_info_request_packet(9);
        assert_ne!(format!("{sign:?}"), format!("{map:?}"));
    }
}
