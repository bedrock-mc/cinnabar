//! Raw packet types for proxies and partial packet inspection.
//!
//! This module provides [`RawPacket`], a packet representation that parses only the
//! header (packet ID + subclient info) while keeping the body as raw bytes.
//! This is useful for:
//! - **Proxies**: Inspect packet IDs without full decode, then forward as-is
//! - **Filtering**: Match on packet type without parse overhead
//! - **Passthrough**: Forward unknown/unimplemented packets transparently

use bytes::{Buf, Bytes, BytesMut};
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::context::BedrockSession;
use valentine::protocol::wire;

use crate::error::{JolyneError, ProtocolError};
use crate::valentine::BorrowedMcpePacket;
use crate::valentine::mcpe::{GameHeader, McpePacket, McpePacketData, McpePacketName};

/// A packet with only the header parsed, body kept as raw bytes.
///
/// Useful for proxies that need to peek at packet IDs without full decode,
/// then forward packets as raw bytes. This avoids the overhead of parsing
/// and re-serializing packet bodies.
///
/// # Example
/// ```ignore
/// match stream.recv_packet_raw().await?.id {
///     McpePacketName::PacketText => { /* snoop on chat */ },
///     _ => stream.send_packet_raw(raw).await?, // forward as-is
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RawPacket {
    /// Parsed header with subclient info.
    pub header: GameHeader,
    /// Packet ID for easy pattern matching.
    pub id: McpePacketName,
    /// Raw body bytes (everything after the varint header).
    /// This does NOT include the length prefix or header varint—just the payload.
    body: Bytes,
    /// Complete inner frame bytes for re-encoding.
    /// Format: `[Length varint][Header varint][Body]`
    inner_frame: Bytes,
}

impl RawPacket {
    /// Decode the raw bytes into a full [`McpePacket`] on demand.
    ///
    /// This is useful when you decide you need the full packet contents
    /// after initially receiving it as raw bytes.
    pub fn decode(self, session: &BedrockSession) -> Result<McpePacket, JolyneError> {
        // Debug: Log raw bytes for TextPacket
        if self.id == McpePacketName::PacketText {
            let body_preview: Vec<u8> = self.body.iter().take(64).copied().collect();
            tracing::warn!(
                packet_id = ?self.id,
                body_len = self.body.len(),
                body_hex = ?body_preview,
                "TextPacket raw bytes before decode"
            );
        }

        let mut buf = self.inner_frame;
        let (header, data) =
            McpePacketData::decode_inner(&mut buf, session.into()).map_err(|e| {
                tracing::error!(
                    packet_id = ?self.id,
                    body_len = self.body.len(),
                    "Failed to decode packet: {:?}",
                    e
                );
                e
            })?;
        Ok(McpePacket::new(header, data))
    }

    /// Decode the inner frame into a borrowed packet view.
    pub fn decode_borrowed(self) -> Result<BorrowedMcpePacket, JolyneError> {
        let mut buf = self.inner_frame;
        Ok(BorrowedMcpePacket::decode_inner(&mut buf)?)
    }

    /// Returns the raw body bytes (payload after header).
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Consumes self and returns the complete inner frame bytes.
    ///
    /// Format: `[Length varint][Header varint][Body]`
    /// Ready for batching/encoding.
    pub fn into_inner_frame(self) -> Bytes {
        self.inner_frame
    }

    /// Returns a reference to the inner frame bytes.
    pub fn inner_frame(&self) -> &Bytes {
        &self.inner_frame
    }
}

/// Decodes a single packet entry from batch payload into a [`RawPacket`].
///
/// Format: `[Length varint][Header varint][Body]`
///
/// Returns the RawPacket and advances the cursor past this entry.
pub fn decode_packet_raw(cursor: &mut Bytes) -> Result<RawPacket, JolyneError> {
    // Remember start position to capture full frame
    let frame_start = cursor.clone();

    // Read length
    let declared_len = wire::read_var_u32(cursor)? as usize;
    if cursor.remaining() < declared_len {
        return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
            format!(
                "declared packet length {} exceeds available {}",
                declared_len,
                cursor.remaining()
            ),
        )));
    }

    // Calculate frame size: length varint + declared payload
    let len_varint_size = frame_start.remaining() - cursor.remaining();
    let frame_size = len_varint_size + declared_len;
    let inner_frame = frame_start.slice(..frame_size);

    // Parse header from payload
    let mut payload = cursor.slice(..declared_len);
    let header_raw = wire::read_var_u32(&mut payload)?;

    // Decode the packet ID from the same varint representation used by generated
    // BedrockCodec impls. Feeding little-endian bytes breaks IDs >= 128 because
    // packet IDs are encoded as VarUInts on the wire.
    let id_raw = header_raw & 0x3FF;
    let mut id_buf = BytesMut::new();
    wire::write_var_u32(&mut id_buf, id_raw);
    let mut id_cursor = id_buf.freeze();
    let id = McpePacketName::decode(&mut id_cursor, ()).map_err(|e| {
        JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
            "unknown packet ID {}: {}",
            id_raw, e
        )))
    })?;

    let from_subclient = (header_raw >> 10) & 0x3;
    let to_subclient = (header_raw >> 12) & 0x3;

    let header = GameHeader {
        id,
        from_subclient,
        to_subclient,
    };

    // Body is the remaining payload after header varint
    let body = payload.clone();

    // Advance main cursor past this packet
    cursor.advance(declared_len);

    Ok(RawPacket {
        header,
        id,
        body,
        inner_frame,
    })
}

/// Decodes a single packet entry from a split frame where the first byte has
/// already been separated from the remaining payload.
///
/// This still materializes a contiguous frame because [`RawPacket`] stores the
/// complete inner frame for later forwarding and lazy decode.
pub fn decode_packet_raw_split(first: u8, rest: Bytes) -> Result<RawPacket, JolyneError> {
    let mut frame = BytesMut::with_capacity(1 + rest.len());
    frame.extend_from_slice(&[first]);
    frame.extend_from_slice(&rest);
    let mut cursor = frame.freeze();
    decode_packet_raw(&mut cursor)
}

/// Decodes all packets from a decompressed batch payload into [`RawPacket`]s.
pub(crate) fn decode_packets_raw(mut cursor: Bytes) -> Result<Vec<RawPacket>, JolyneError> {
    let mut packets = Vec::new();
    while cursor.has_remaining() {
        packets.push(decode_packet_raw(&mut cursor)?);
    }
    Ok(packets)
}

/// Encodes a slice of [`RawPacket`]s into batch payload bytes.
///
/// This just concatenates the inner frames—caller handles compression/batching.
pub(crate) fn encode_packets_raw(packets: &[RawPacket]) -> Bytes {
    let total_len: usize = packets.iter().map(|p| p.inner_frame.len()).sum();
    let mut buf = BytesMut::with_capacity(total_len);
    for packet in packets {
        buf.extend_from_slice(&packet.inner_frame);
    }
    buf.freeze()
}

#[cfg(test)]
mod tests {
    use super::*;
    use valentine::protocol::wire;

    // ========== Header Parsing Tests ==========

    #[test]
    fn header_extracts_10bit_packet_id() {
        // Header format: [10-bit packet_id][2-bit from_subclient][2-bit to_subclient]
        // Max 10-bit value is 1023
        let packet_id: u32 = 0x09; // PlayStatus packet
        let header_raw = packet_id; // No subclient bits

        let extracted_id = header_raw & 0x3FF;
        assert_eq!(extracted_id, 0x09);
    }

    #[test]
    fn header_extracts_subclient_bits() {
        // from_subclient is bits 10-11, to_subclient is bits 12-13
        let packet_id: u32 = 0x09;
        let from_sub: u32 = 2; // bits 10-11
        let to_sub: u32 = 3; // bits 12-13

        let header_raw = packet_id | (from_sub << 10) | (to_sub << 12);

        let extracted_id = header_raw & 0x3FF;
        let extracted_from = (header_raw >> 10) & 0x3;
        let extracted_to = (header_raw >> 12) & 0x3;

        assert_eq!(extracted_id, 0x09);
        assert_eq!(extracted_from, 2);
        assert_eq!(extracted_to, 3);
    }

    #[test]
    fn subclient_values_are_2bits() {
        // Each subclient field is 2 bits, so values 0-3 are valid
        for value in 0u32..=3 {
            assert!(value <= 0x3);
        }
    }

    // ========== VarInt Length Encoding Tests ==========

    #[test]
    fn varint_single_byte_lengths() {
        // Values 0-127 encode as single byte
        for len in [0u32, 1, 64, 127] {
            let mut buf = BytesMut::new();
            wire::write_var_u32(&mut buf, len);
            assert_eq!(buf.len(), 1);
        }
    }

    #[test]
    fn varint_two_byte_lengths() {
        // Values 128-16383 encode as two bytes
        for len in [128u32, 256, 1000, 16383] {
            let mut buf = BytesMut::new();
            wire::write_var_u32(&mut buf, len);
            assert_eq!(buf.len(), 2);
        }
    }

    #[test]
    fn varint_three_byte_lengths() {
        // Values 16384-2097151 encode as three bytes
        for len in [16384u32, 100000, 2097151] {
            let mut buf = BytesMut::new();
            wire::write_var_u32(&mut buf, len);
            assert_eq!(buf.len(), 3);
        }
    }

    // ========== Frame Format Tests ==========

    #[test]
    fn frame_format_length_header_body() {
        // Create a simple frame: [Length varint][Header varint][Body]
        let packet_id: u32 = 0x09; // PlayStatus
        let body = &[0x00, 0x00, 0x00, 0x01]; // status = 1

        let mut payload = BytesMut::new();
        wire::write_var_u32(&mut payload, packet_id);
        payload.extend_from_slice(body);

        let payload_len = payload.len() as u32;

        let mut frame = BytesMut::new();
        wire::write_var_u32(&mut frame, payload_len);
        frame.extend_from_slice(&payload);

        // Frame should start with length varint
        let mut cursor = frame.freeze();
        let read_len = wire::read_var_u32(&mut cursor).expect("read length");
        assert_eq!(read_len, payload_len);
    }

    // ========== decode_packet_raw Tests ==========

    fn create_test_frame(packet_id: u32, from_sub: u32, to_sub: u32, body: &[u8]) -> Bytes {
        let header_raw = packet_id | (from_sub << 10) | (to_sub << 12);

        let mut payload = BytesMut::new();
        wire::write_var_u32(&mut payload, header_raw);
        payload.extend_from_slice(body);

        let mut frame = BytesMut::new();
        wire::write_var_u32(&mut frame, payload.len() as u32);
        frame.extend_from_slice(&payload);

        frame.freeze()
    }

    #[test]
    fn decode_packet_raw_parses_header() {
        // PacketPlayStatus = 2
        let frame = create_test_frame(0x02, 1, 2, &[0x00, 0x00, 0x00, 0x01]);
        let mut cursor = frame.clone();

        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert_eq!(raw.id, McpePacketName::PacketPlayStatus);
        assert_eq!(raw.header.from_subclient, 1);
        assert_eq!(raw.header.to_subclient, 2);
    }

    #[test]
    fn decode_packet_raw_parses_multibyte_packet_id() {
        let frame = create_test_frame(193, 0, 0, &[]);
        let mut cursor = frame;

        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert_eq!(raw.id, McpePacketName::PacketRequestNetworkSettings);
    }

    #[test]
    fn decode_packet_raw_preserves_body() {
        let body = [0xDE, 0xAD, 0xBE, 0xEF];
        let frame = create_test_frame(0x02, 0, 0, &body); // PacketPlayStatus = 2
        let mut cursor = frame.clone();

        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert_eq!(raw.body().as_ref(), &body);
    }

    #[test]
    fn decode_packet_raw_preserves_inner_frame() {
        let frame = create_test_frame(0x02, 0, 0, &[0x01, 0x02, 0x03]); // PacketPlayStatus = 2
        let mut cursor = frame.clone();

        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert_eq!(raw.inner_frame(), &frame);
    }

    #[test]
    fn decode_packet_raw_advances_cursor() {
        let frame1 = create_test_frame(0x02, 0, 0, &[0x01]); // PacketPlayStatus = 2
        let frame2 = create_test_frame(0x02, 0, 0, &[0x02]);

        let mut combined = BytesMut::new();
        combined.extend_from_slice(&frame1);
        combined.extend_from_slice(&frame2);
        let mut cursor = combined.freeze();

        let raw1 = decode_packet_raw(&mut cursor).expect("decode first");
        assert_eq!(raw1.body().as_ref(), &[0x01]);

        let raw2 = decode_packet_raw(&mut cursor).expect("decode second");
        assert_eq!(raw2.body().as_ref(), &[0x02]);

        assert!(!cursor.has_remaining(), "cursor should be exhausted");
    }

    #[test]
    fn decode_packet_raw_rejects_truncated_length() {
        // Truncated varint for length
        let mut cursor = Bytes::from_static(&[0x80]); // Incomplete varint
        let result = decode_packet_raw(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn decode_packet_raw_rejects_length_exceeding_buffer() {
        // Length claims 100 bytes, but we only have 5
        let mut frame = BytesMut::new();
        wire::write_var_u32(&mut frame, 100);
        frame.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05]);

        let mut cursor = frame.freeze();
        let result = decode_packet_raw(&mut cursor);
        assert!(result.is_err());
    }

    // ========== decode_packets_raw Tests ==========

    #[test]
    fn decode_packets_raw_handles_empty() {
        let cursor = Bytes::new();
        let packets = decode_packets_raw(cursor).expect("decode empty");
        assert!(packets.is_empty());
    }

    #[test]
    fn decode_packets_raw_multiple_packets() {
        let frame1 = create_test_frame(0x02, 0, 0, &[0x01]); // PacketPlayStatus = 2
        let frame2 = create_test_frame(0x02, 1, 1, &[0x02]);
        let frame3 = create_test_frame(0x02, 2, 2, &[0x03]);

        let mut combined = BytesMut::new();
        combined.extend_from_slice(&frame1);
        combined.extend_from_slice(&frame2);
        combined.extend_from_slice(&frame3);

        let packets = decode_packets_raw(combined.freeze()).expect("decode");
        assert_eq!(packets.len(), 3);

        assert_eq!(packets[0].body().as_ref(), &[0x01]);
        assert_eq!(packets[1].header.from_subclient, 1);
        assert_eq!(packets[2].header.to_subclient, 2);
    }

    // ========== encode_packets_raw Tests ==========

    #[test]
    fn encode_packets_raw_empty_returns_empty() {
        let result = encode_packets_raw(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn encode_packets_raw_roundtrip() {
        let frame1 = create_test_frame(0x02, 0, 0, &[0x01, 0x02]); // PacketPlayStatus = 2
        let frame2 = create_test_frame(0x02, 1, 1, &[0x03, 0x04]);

        // Decode
        let mut combined = BytesMut::new();
        combined.extend_from_slice(&frame1);
        combined.extend_from_slice(&frame2);
        let packets = decode_packets_raw(combined.freeze()).expect("decode");

        // Re-encode
        let encoded = encode_packets_raw(&packets);

        // Decode again
        let packets2 = decode_packets_raw(encoded).expect("decode again");

        assert_eq!(packets.len(), packets2.len());
        for (p1, p2) in packets.iter().zip(packets2.iter()) {
            assert_eq!(p1.id, p2.id);
            assert_eq!(p1.body(), p2.body());
            assert_eq!(p1.inner_frame(), p2.inner_frame());
        }
    }

    // ========== RawPacket Method Tests ==========

    #[test]
    fn raw_packet_body_accessor() {
        let frame = create_test_frame(0x02, 0, 0, &[0xAA, 0xBB, 0xCC]); // PacketPlayStatus = 2
        let mut cursor = frame;

        let raw = decode_packet_raw(&mut cursor).expect("decode");
        let body = raw.body();

        assert_eq!(body.as_ref(), &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn raw_packet_inner_frame_accessor() {
        let frame = create_test_frame(0x02, 0, 0, &[0x01]); // PacketPlayStatus = 2
        let original_frame = frame.clone();
        let mut cursor = frame;

        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert_eq!(raw.inner_frame(), &original_frame);
    }

    #[test]
    fn raw_packet_into_inner_frame_consumes() {
        let frame = create_test_frame(0x02, 0, 0, &[0x01, 0x02]); // PacketPlayStatus = 2
        let original_frame = frame.clone();
        let mut cursor = frame;

        let raw = decode_packet_raw(&mut cursor).expect("decode");
        let inner = raw.into_inner_frame();

        assert_eq!(inner, original_frame);
    }

    #[test]
    fn raw_packet_clone() {
        let frame = create_test_frame(0x02, 1, 2, &[0x01, 0x02, 0x03]); // PacketPlayStatus = 2
        let mut cursor = frame;

        let raw = decode_packet_raw(&mut cursor).expect("decode");
        let cloned = raw.clone();

        assert_eq!(raw.id, cloned.id);
        assert_eq!(raw.header.from_subclient, cloned.header.from_subclient);
        assert_eq!(raw.header.to_subclient, cloned.header.to_subclient);
        assert_eq!(raw.body(), cloned.body());
        assert_eq!(raw.inner_frame(), cloned.inner_frame());
    }

    // ========== Known Packet ID Tests ==========

    #[test]
    fn decode_known_packet_ids() {
        // Test a few known packet IDs
        // PacketLogin = 1, PacketPlayStatus = 2, PacketDisconnect = 5
        let known_ids: [(u32, McpePacketName); 3] = [
            (0x01, McpePacketName::PacketLogin),
            (0x02, McpePacketName::PacketPlayStatus),
            (0x05, McpePacketName::PacketDisconnect),
        ];

        for (id, expected_name) in known_ids {
            let frame = create_test_frame(id, 0, 0, &[0x00]);
            let mut cursor = frame;

            let raw = decode_packet_raw(&mut cursor).expect("decode");
            assert_eq!(raw.id, expected_name, "packet id 0x{:02X}", id);
        }
    }

    // ========== Edge Cases ==========

    #[test]
    fn decode_empty_body() {
        // A packet with just a header, no body
        let packet_id: u32 = 0x02; // PacketPlayStatus = 2
        let header_raw = packet_id;

        let mut payload = BytesMut::new();
        wire::write_var_u32(&mut payload, header_raw);
        // No body

        let mut frame = BytesMut::new();
        wire::write_var_u32(&mut frame, payload.len() as u32);
        frame.extend_from_slice(&payload);

        let mut cursor = frame.freeze();
        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert!(raw.body().is_empty());
    }

    #[test]
    fn decode_large_body() {
        // 1KB body
        let body: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        let frame = create_test_frame(0x02, 0, 0, &body); // PacketPlayStatus = 2
        let mut cursor = frame;

        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert_eq!(raw.body().len(), 1024);
        assert_eq!(raw.body().as_ref(), body.as_slice());
    }

    #[test]
    fn decode_max_subclient_values() {
        // Max subclient values (0x3 = 3)
        let frame = create_test_frame(0x02, 3, 3, &[0x01]); // PacketPlayStatus = 2
        let mut cursor = frame;

        let raw = decode_packet_raw(&mut cursor).expect("decode");

        assert_eq!(raw.header.from_subclient, 3);
        assert_eq!(raw.header.to_subclient, 3);
    }
}
