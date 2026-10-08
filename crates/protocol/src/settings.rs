//! Client settings that need a live Bedrock request.

use crate::Packet;
use valentine::bedrock::version::v1_26_51::RequestChunkRadiusPacket;

// Vanilla advertises a supported chunk radius within these bounds.
const MIN_ADVERTISED_CHUNK_RADIUS: u8 = 5;
const MAX_ADVERTISED_CHUNK_RADIUS: u8 = 28;

/// Requests a live view radius, advertising the client's supported upper bound.
#[must_use]
pub fn request_chunk_radius_packet(chunks: u8, supported_chunks: u8) -> Packet {
    RequestChunkRadiusPacket {
        chunk_radius: i32::from(chunks.min(supported_chunks)),
        max_chunk_radius: supported_chunks
            .clamp(MIN_ADVERTISED_CHUNK_RADIUS, MAX_ADVERTISED_CHUNK_RADIUS),
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use valentine::bedrock::version::v1_26_51::McpePacketData;

    #[test]
    fn live_radius_requests_encode_both_requested_and_supported_distance() {
        let packet = request_chunk_radius_packet(8, 16);
        let session = crate::BedrockSession { shield_item_id: 0 };
        let encoded = crate::encode(&packet, &session).unwrap();
        let decoded = crate::decode_batch(encoded, &session).unwrap();
        let McpePacketData::RequestChunkRadiusPacket(request) = &decoded[0].data else {
            panic!("wrong packet");
        };
        assert_eq!(request.chunk_radius, 8);
        assert_eq!(request.max_chunk_radius, 16);
        let packet = request_chunk_radius_packet(100, 40);
        let McpePacketData::RequestChunkRadiusPacket(request) = packet.data else {
            panic!("wrong packet");
        };
        assert_eq!(request.chunk_radius, 40);
        assert_eq!(request.max_chunk_radius, MAX_ADVERTISED_CHUNK_RADIUS);
    }
}
