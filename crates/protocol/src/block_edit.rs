//! Outbound packets for block-side requests: sign edits and map image requests.

use bedrock_protocol_1_26_51::packets::{BlockActorData, MapInfoRequest};
use bedrock_protocol_1_26_51::types::{ActorUniqueID, BlockPos};
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;

use crate::Packet;

/// The `BlockActorData` a client sends when it closes a sign editor; `nbt` is the whole sign
/// compound as NetworkLittleEndian bytes.
#[must_use]
pub fn sign_edit_packet(position: [i32; 3], nbt: &[u8]) -> Packet {
    crate::Packet::new(BlockActorData {
        block_position: BlockPos {
            x: wire::ZigZag32(position[0]),
            y: wire::ZigZag32(position[1]),
            z: wire::ZigZag32(position[2]),
        },
        actor_data_tags: wire::NetworkNbt(Bytes::copy_from_slice(nbt)),
    })
}

/// Asks the server for the pixels of `map_id`, as a client does on first sight of a map.
#[must_use]
pub fn map_info_request_packet(map_id: i64) -> Packet {
    crate::Packet::new(MapInfoRequest {
        map_unique_id: ActorUniqueID(map_id),
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
