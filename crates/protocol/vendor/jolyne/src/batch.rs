#[instrument(skip(cursor, session), level = "trace")]
fn decode_packets(
    mut cursor: Bytes,
    session: &BedrockSession,
) -> Result<Vec<McpePacket>, JolyneError> {
    let mut packets = Vec::new();
    while cursor.has_remaining() {
        let (header, data) = McpePacketData::decode_inner(&mut cursor, session.into())?;
        packets.push(McpePacket { header, data });
    }
    Ok(packets)
}

use crate::error::{JolyneError, ProtocolError};
use crate::valentine::mcpe::{GAME_PACKET_ID as GAME_FRAME_ID, McpePacket, McpePacketData};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use std::io::{ErrorKind, Read};
use std::slice;
use tracing::{instrument, trace, warn};
use valentine::bedrock::codec::BedrockSized;
use valentine::bedrock::context::BedrockSession;
use valentine::protocol::wire as bedrock_wire;

pub const BATCH_PACKET_ID: u8 = 0xFE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BatchCompression {
    Deflate = 0x00,
    Snappy = 0x01,
    None = 0xFF,
}

impl BatchCompression {
    pub fn try_from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Deflate),
            0x01 => Some(Self::Snappy),
            0xFF => Some(Self::None),
            _ => None,
        }
    }
}

/// Helper to stream decompress while enforcing an optional maximum output size.
fn decompress_with_guard<R: Read>(
    mut reader: R,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<u8>, std::io::Error> {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let new_len = out.len() + n;
        if let Some(max) = max_decompressed_size
            && new_len > max
        {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("decompressed data exceeds max size of {max} bytes"),
            ));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

fn decompress_snappy_with_guard(
    input: &[u8],
    max_decompressed_size: Option<usize>,
) -> Result<Vec<u8>, ProtocolError> {
    let len = snap::raw::decompress_len(input)
        .map_err(|e| ProtocolError::DecompressionFailed(e.to_string()))?;
    if let Some(max) = max_decompressed_size
        && len > max
    {
        return Err(ProtocolError::DecompressionFailed(format!(
            "decompressed data exceeds max size of {max} bytes"
        )));
    }

    snap::raw::Decoder::new()
        .decompress_vec(input)
        .map_err(|e| ProtocolError::DecompressionFailed(e.to_string()))
}

fn log_payload_probe(compressed_len: Option<usize>, payload: &Bytes) {
    let preview_len = payload.len().min(16);
    let preview: Vec<u8> = payload.iter().take(preview_len).copied().collect();
    let first_declared_len = {
        let mut tmp = payload.clone();
        bedrock_wire::read_var_u32(&mut tmp).ok()
    };
    trace!(
        compressed_len,
        decompressed_len = payload.len(),
        first_bytes = ?preview,
        first_declared_packet_len = first_declared_len,
        "decode_batch payload probe"
    );
}

fn decode_payload(
    payload: Bytes,
    session: &BedrockSession,
) -> Result<Vec<McpePacket>, JolyneError> {
    if payload.first().copied() == Some(GAME_FRAME_ID) {
        let mut buf = payload.clone();
        let (header, data) = McpePacketData::decode_game_frame(&mut buf, session.into())?;
        return Ok(vec![McpePacket { header, data }]);
    }
    decode_packets(payload, session)
}

/// Decodes a Batch Packet (0xFE) payload into a list of McpePackets.
#[instrument(skip_all, level = "trace")]
pub fn decode_batch(
    buf: &mut Bytes,
    session: &BedrockSession,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<McpePacket>, JolyneError> {
    if buf.is_empty() {
        return Ok(vec![]);
    }

    let packet_id = buf.get_u8();
    if packet_id != BATCH_PACKET_ID {
        return Err(ProtocolError::InvalidBatchId(format!(
            "expected 0xFE, got 0x{:02x}",
            packet_id
        ))
        .into());
    }

    let payload_raw = buf.clone();

    // Strict Mode: If compression is enabled, we EXPECT a valid algorithm byte.
    if compression_enabled {
        if payload_raw.is_empty() {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Empty compressed batch payload".to_string(),
            )));
        }

        let alg_byte = payload_raw[0];
        let alg = BatchCompression::try_from_u8(alg_byte).ok_or_else(|| {
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
                "Unknown compression algorithm: 0x{:02x} ({})",
                alg_byte, alg_byte
            )))
        })?;

        let compressed = payload_raw.slice(1..);

        // Size Check
        if let Some(max) = max_decompressed_size {
            // Rough check: compressed size shouldn't exceed max (it usually shrinks, but overhead exists)
            // Ideally we check *decompressed* size during stream.
            if compressed.len() > max {
                warn!("Compressed payload large: {}", compressed.len());
            }
        }

        match alg {
            BatchCompression::Deflate => {
                let decompressed = decompress_with_guard(
                    DeflateDecoder::new(compressed.as_ref()),
                    max_decompressed_size,
                )
                .map_err(|e| ProtocolError::DecompressionFailed(e.to_string()))?;

                let payload = Bytes::from(decompressed);
                log_payload_probe(Some(compressed.len()), &payload);
                decode_payload(payload, session)
            }
            BatchCompression::None => {
                log_payload_probe(Some(compressed.len()), &compressed);
                decode_payload(compressed, session)
            }
            BatchCompression::Snappy => {
                let decompressed =
                    decompress_snappy_with_guard(compressed.as_ref(), max_decompressed_size)?;
                let payload = Bytes::from(decompressed);
                log_payload_probe(Some(compressed.len()), &payload);
                decode_payload(payload, session)
            }
        }
    } else {
        // raw packets (before NetworkSettings) are just [0xFE] [Payload].

        if let Some(max) = max_decompressed_size
            && payload_raw.len() > max
        {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                format!(
                    "Batch payload exceeds max decompressed size ({} > {})",
                    payload_raw.len(),
                    max
                ),
            )));
        }
        log_payload_probe(None, &payload_raw);
        decode_payload(payload_raw, session)
    }
}

/// Decodes a batch packet WITHOUT the 0xFE prefix (for NetherNet).
///
/// NetherNet batch format is: `[CompressionAlg][CompressedData]`
/// rather than RakNet's: `[0xFE][CompressionAlg][CompressedData]`
#[instrument(skip_all, level = "trace")]
pub fn decode_batch_no_prefix(
    buf: &mut Bytes,
    session: &BedrockSession,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<McpePacket>, JolyneError> {
    if buf.is_empty() {
        return Ok(vec![]);
    }

    // Same as decode_batch but without consuming the 0xFE prefix
    let alg_byte = buf.get_u8();
    let alg = BatchCompression::try_from_u8(alg_byte).ok_or_else(|| {
        JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
            "Unknown compression algorithm: 0x{:02x} ({})",
            alg_byte, alg_byte
        )))
    })?;

    let compressed = buf.clone();

    // Size check
    if let Some(max) = max_decompressed_size
        && compressed.len() > max
    {
        warn!("Compressed payload large: {}", compressed.len());
    }

    match alg {
        BatchCompression::Deflate => {
            let decompressed = decompress_with_guard(
                DeflateDecoder::new(compressed.as_ref()),
                max_decompressed_size,
            )
            .map_err(|e| ProtocolError::DecompressionFailed(e.to_string()))?;

            let payload = Bytes::from(decompressed);
            log_payload_probe(Some(compressed.len()), &payload);
            decode_payload(payload, session)
        }
        BatchCompression::None => {
            log_payload_probe(Some(compressed.len()), &compressed);
            decode_payload(compressed, session)
        }
        BatchCompression::Snappy => {
            let decompressed =
                decompress_snappy_with_guard(compressed.as_ref(), max_decompressed_size)?;
            let payload = Bytes::from(decompressed);
            log_payload_probe(Some(compressed.len()), &payload);
            decode_payload(payload, session)
        }
    }
}

/// Encodes a single packet into a Batch Packet.
pub fn encode_batch(
    packet: &McpePacket,
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
) -> Result<Bytes, JolyneError> {
    encode_batch_multi(
        slice::from_ref(packet),
        compression_enabled,
        compression_level,
        compression_threshold,
        true, // RakNet-style with 0xFE prefix
    )
}

/// Encodes multiple packets into a single Batch Packet.
///
/// - `use_batch_prefix`: If true, prepends 0xFE (RakNet). If false, skips it (NetherNet).
pub fn encode_batch_multi(
    packets: &[McpePacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<Bytes, JolyneError> {
    Ok(encode_batch_multi_bytes_mut(
        packets,
        compression_enabled,
        compression_level,
        compression_threshold,
        use_batch_prefix,
    )?
    .freeze())
}

pub(crate) fn encode_batch_multi_bytes_mut(
    packets: &[McpePacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<BytesMut, JolyneError> {
    let mut out = BytesMut::new();
    encode_batch_multi_into(
        packets,
        compression_enabled,
        compression_level,
        compression_threshold,
        use_batch_prefix,
        &mut out,
    )?;
    Ok(out)
}

pub(crate) fn encode_batch_multi_into(
    packets: &[McpePacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
    out: &mut BytesMut,
) -> Result<(), JolyneError> {
    let estimated_len = packets
        .iter()
        .map(|packet| 10usize + packet.data.encoded_size())
        .sum();
    out.clear();
    out.reserve(estimated_len);
    for packet in packets {
        packet.data.encode_inner_bytes_mut(
            out,
            packet.header.from_subclient,
            packet.header.to_subclient,
        )?;
    }

    let payload = if compression_enabled {
        let should_compress = compression_level > 0 && out.len() >= compression_threshold as usize;
        if should_compress {
            // Deflate (0x00)
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(compression_level));
            std::io::Write::write_all(&mut encoder, out.as_ref()).map_err(JolyneError::Io)?;
            let compressed = encoder.finish().map_err(JolyneError::Io)?;

            let mut out = BytesMut::with_capacity(1 + compressed.len());
            out.put_u8(BatchCompression::Deflate as u8);
            out.extend_from_slice(&compressed);
            out
        } else {
            // None (0xFF)
            let uncompressed = out.split_off(0);
            let mut out = BytesMut::with_capacity(1 + uncompressed.len());
            out.put_u8(BatchCompression::None as u8);
            out.extend_from_slice(&uncompressed);
            out
        }
    } else {
        // No Marker
        out.split_off(0)
    };

    out.clear();
    if use_batch_prefix {
        // RakNet: [0xFE][payload]
        out.reserve(1 + payload.len());
        out.put_u8(BATCH_PACKET_ID);
        out.extend_from_slice(&payload);
    } else {
        // NetherNet: [payload] (no 0xFE prefix)
        out.reserve(payload.len());
        out.extend_from_slice(&payload);
    }
    Ok(())
}

// ============================================================================
// Raw Packet Batch Decoding (ID-only parse, body as bytes)
// ============================================================================

use crate::raw::{RawPacket, decode_packets_raw, encode_packets_raw};

/// Decodes a batch packet (0xFE prefix) into [`RawPacket`]s.
///
/// Only parses packet IDs—bodies are kept as raw bytes.
/// Ideal for proxies that need to inspect packet types without full decode.
#[instrument(skip_all, level = "trace")]
pub fn decode_batch_raw(
    buf: &mut Bytes,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<RawPacket>, JolyneError> {
    if buf.is_empty() {
        return Ok(vec![]);
    }

    let packet_id = buf.get_u8();
    if packet_id != BATCH_PACKET_ID {
        return Err(ProtocolError::InvalidBatchId(format!(
            "expected 0xFE, got 0x{:02x}",
            packet_id
        ))
        .into());
    }

    decode_batch_payload_raw(buf.clone(), compression_enabled, max_decompressed_size)
}

/// Decodes a RakNet batch/raw frame when the first byte has already been split
/// from the remaining payload by the underlying transport.
#[instrument(skip_all, level = "trace")]
pub fn decode_batch_raw_split(
    first: u8,
    payload_raw: Bytes,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<RawPacket>, JolyneError> {
    if first != BATCH_PACKET_ID {
        return Err(
            ProtocolError::InvalidBatchId(format!("expected 0xFE, got 0x{:02x}", first)).into(),
        );
    }

    if compression_enabled {
        if payload_raw.is_empty() {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Empty compressed batch payload".to_string(),
            )));
        }

        let alg_byte = payload_raw[0];
        let alg = BatchCompression::try_from_u8(alg_byte).ok_or_else(|| {
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
                "Unknown compression algorithm: 0x{:02x} ({})",
                alg_byte, alg_byte
            )))
        })?;

        let compressed = payload_raw.slice(1..);

        if let Some(max) = max_decompressed_size
            && compressed.len() > max
        {
            warn!("Compressed payload large: {}", compressed.len());
        }

        match alg {
            BatchCompression::Deflate => {
                let decompressed = decompress_with_guard(
                    DeflateDecoder::new(compressed.as_ref()),
                    max_decompressed_size,
                )
                .map_err(|e| ProtocolError::DecompressionFailed(e.to_string()))?;
                decode_packets_raw(Bytes::from(decompressed))
            }
            BatchCompression::None => decode_packets_raw(compressed),
            BatchCompression::Snappy => {
                let decompressed =
                    decompress_snappy_with_guard(compressed.as_ref(), max_decompressed_size)?;
                decode_packets_raw(Bytes::from(decompressed))
            }
        }
    } else {
        if let Some(max) = max_decompressed_size
            && payload_raw.len() > max
        {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                format!(
                    "Batch payload exceeds max decompressed size ({} > {})",
                    payload_raw.len(),
                    max
                ),
            )));
        }
        decode_packets_raw(payload_raw)
    }
}

/// Decodes a batch packet WITHOUT the 0xFE prefix (NetherNet) into [`RawPacket`]s.
#[instrument(skip_all, level = "trace")]
pub fn decode_batch_no_prefix_raw(
    buf: &mut Bytes,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<RawPacket>, JolyneError> {
    if buf.is_empty() {
        return Ok(vec![]);
    }

    // NetherNet always uses compression format after NetworkSettings
    decode_batch_payload_raw(buf.clone(), true, max_decompressed_size)
}

/// Internal helper: decompress and decode payload into RawPackets.
fn decode_batch_payload_raw(
    payload_raw: Bytes,
    compression_enabled: bool,
    max_decompressed_size: Option<usize>,
) -> Result<Vec<RawPacket>, JolyneError> {
    if compression_enabled {
        if payload_raw.is_empty() {
            return Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(
                "Empty compressed batch payload".to_string(),
            )));
        }

        let alg_byte = payload_raw[0];
        let alg = BatchCompression::try_from_u8(alg_byte).ok_or_else(|| {
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(format!(
                "Unknown compression algorithm: 0x{:02x} ({})",
                alg_byte, alg_byte
            )))
        })?;

        let compressed = payload_raw.slice(1..);

        match alg {
            BatchCompression::Deflate => {
                let decompressed = decompress_with_guard(
                    DeflateDecoder::new(compressed.as_ref()),
                    max_decompressed_size,
                )
                .map_err(|e| ProtocolError::DecompressionFailed(e.to_string()))?;

                decode_packets_raw(Bytes::from(decompressed))
            }
            BatchCompression::None => decode_packets_raw(compressed),
            BatchCompression::Snappy => {
                let decompressed =
                    decompress_snappy_with_guard(compressed.as_ref(), max_decompressed_size)?;
                decode_packets_raw(Bytes::from(decompressed))
            }
        }
    } else {
        decode_packets_raw(payload_raw)
    }
}

/// Encodes [`RawPacket`]s into a batch with optional compression.
///
/// - `use_batch_prefix`: If true, prepends 0xFE (RakNet). If false, skips it (NetherNet).
pub fn encode_batch_raw(
    packets: &[RawPacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<Bytes, JolyneError> {
    Ok(encode_batch_raw_bytes_mut(
        packets,
        compression_enabled,
        compression_level,
        compression_threshold,
        use_batch_prefix,
    )?
    .freeze())
}

pub(crate) fn encode_batch_raw_bytes_mut(
    packets: &[RawPacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
) -> Result<BytesMut, JolyneError> {
    let mut out = BytesMut::new();
    encode_batch_raw_into(
        packets,
        compression_enabled,
        compression_level,
        compression_threshold,
        use_batch_prefix,
        &mut out,
    )?;
    Ok(out)
}

pub(crate) fn encode_batch_raw_into(
    packets: &[RawPacket],
    compression_enabled: bool,
    compression_level: u32,
    compression_threshold: u16,
    use_batch_prefix: bool,
    out: &mut BytesMut,
) -> Result<(), JolyneError> {
    let uncompressed = encode_packets_raw(packets);

    let should_compress = compression_enabled
        && compression_level > 0
        && uncompressed.len() >= compression_threshold as usize;

    let payload = if compression_enabled {
        if should_compress {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(compression_level));
            std::io::Write::write_all(&mut encoder, &uncompressed).map_err(JolyneError::Io)?;
            let compressed = encoder.finish().map_err(JolyneError::Io)?;

            let mut out = BytesMut::with_capacity(1 + compressed.len());
            out.put_u8(BatchCompression::Deflate as u8);
            out.extend_from_slice(&compressed);
            out
        } else {
            let mut out = BytesMut::with_capacity(1 + uncompressed.len());
            out.put_u8(BatchCompression::None as u8);
            out.extend_from_slice(&uncompressed);
            out
        }
    } else {
        BytesMut::from(uncompressed.as_ref())
    };

    out.clear();
    if use_batch_prefix {
        out.reserve(1 + payload.len());
        out.put_u8(BATCH_PACKET_ID);
        out.extend_from_slice(&payload);
    } else {
        out.reserve(payload.len());
        out.extend_from_slice(&payload);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::valentine::{PlayStatusPacket, PlayStatusPacketStatus};

    fn test_session() -> BedrockSession {
        BedrockSession { shield_item_id: 0 }
    }

    #[test]
    fn decode_batch_rejects_wrong_id() {
        let mut buf = Bytes::from_static(&[0x00, 0x01, 0x02]);
        let session = test_session();
        let err = decode_batch(&mut buf, &session, false, None).expect_err("should fail");
        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::InvalidBatchId(_))
        ));
    }

    #[test]
    fn decode_batch_rejects_empty_compressed_payload() {
        let mut buf = Bytes::from_static(&[BATCH_PACKET_ID]);
        let session = test_session();
        // Valid compression enabled -> expects alg byte. Empty -> fail.
        let err = decode_batch(&mut buf, &session, true, None).expect_err("should fail");
        assert!(matches!(
            err,
            JolyneError::Protocol(ProtocolError::UnexpectedHandshake(_))
        ));
    }

    #[test]
    fn decode_batch_rejects_unknown_alg() {
        let mut buf = Bytes::from_static(&[BATCH_PACKET_ID, 0xBA, 0x00]); // 0xBA = 186
        let session = test_session();
        let err = decode_batch(&mut buf, &session, true, None).expect_err("should fail");
        // Should catch our new explicit check
        if let JolyneError::Protocol(ProtocolError::UnexpectedHandshake(msg)) = err {
            assert!(msg.contains("Unknown compression algorithm: 0xba"));
        } else {
            panic!("Wrong error type: {:?}", err);
        }
    }

    #[test]
    fn encode_decode_roundtrip_compressed() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::LoginSuccess,
        });

        let batch = encode_batch(&packet, true, 7, 0).expect("encode");
        let mut buf = batch.clone();

        let decoded = decode_batch(&mut buf, &session, true, Some(1024)).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            decoded[0].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PlayStatusPacketStatus::LoginSuccess
        ));
    }

    // ========== Compression Tests ==========

    #[test]
    fn batch_compression_enum_values() {
        assert_eq!(BatchCompression::Deflate as u8, 0x00);
        assert_eq!(BatchCompression::Snappy as u8, 0x01);
        assert_eq!(BatchCompression::None as u8, 0xFF);
    }

    #[test]
    fn batch_compression_try_from_u8() {
        assert_eq!(
            BatchCompression::try_from_u8(0x00),
            Some(BatchCompression::Deflate)
        );
        assert_eq!(
            BatchCompression::try_from_u8(0x01),
            Some(BatchCompression::Snappy)
        );
        assert_eq!(
            BatchCompression::try_from_u8(0xFF),
            Some(BatchCompression::None)
        );
        assert_eq!(BatchCompression::try_from_u8(0x02), None);
        assert_eq!(BatchCompression::try_from_u8(0xFE), None);
    }

    #[test]
    fn encode_decode_roundtrip_uncompressed() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::PlayerSpawn,
        });

        // Compression disabled
        let batch = encode_batch(&packet, false, 0, 0).expect("encode");
        let mut buf = batch.clone();

        // Should start with 0xFE
        assert_eq!(buf[0], BATCH_PACKET_ID);

        let decoded = decode_batch(&mut buf, &session, false, None).expect("decode");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            decoded[0].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PlayStatusPacketStatus::PlayerSpawn
        ));
    }

    #[test]
    fn encode_with_compression_none_marker() {
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::LoginSuccess,
        });

        // Compression enabled but level 0 or below threshold -> should use None marker
        let batch = encode_batch(&packet, true, 0, 9999).expect("encode");

        // Should be: [0xFE][0xFF][payload]
        assert_eq!(batch[0], BATCH_PACKET_ID);
        assert_eq!(batch[1], BatchCompression::None as u8);
    }

    #[test]
    fn compression_threshold_below_threshold_uses_none() {
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::LoginSuccess,
        });

        // Threshold = 512, packet is small -> should use None compression
        let batch = encode_batch(&packet, true, 7, 512).expect("encode");

        assert_eq!(batch[0], BATCH_PACKET_ID);
        assert_eq!(batch[1], BatchCompression::None as u8);
    }

    #[test]
    fn compression_threshold_above_threshold_uses_deflate() {
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::LoginSuccess,
        });

        // Threshold = 0, any packet should compress
        let batch = encode_batch(&packet, true, 7, 0).expect("encode");

        assert_eq!(batch[0], BATCH_PACKET_ID);
        assert_eq!(batch[1], BatchCompression::Deflate as u8);
    }

    #[test]
    fn decode_batch_empty_returns_empty() {
        let session = test_session();
        let mut buf = Bytes::new();

        let decoded = decode_batch(&mut buf, &session, false, None).expect("decode empty");
        assert!(decoded.is_empty());
    }

    #[test]
    fn multiple_packets_in_batch() {
        let session = test_session();
        let packets = vec![
            McpePacket::from(PlayStatusPacket {
                status: PlayStatusPacketStatus::LoginSuccess,
            }),
            McpePacket::from(PlayStatusPacket {
                status: PlayStatusPacketStatus::PlayerSpawn,
            }),
        ];

        let batch = encode_batch_multi(&packets, true, 7, 0, true).expect("encode multi");
        let mut buf = batch.clone();

        let decoded = decode_batch(&mut buf, &session, true, None).expect("decode");
        assert_eq!(decoded.len(), 2);

        assert!(matches!(
            decoded[0].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PlayStatusPacketStatus::LoginSuccess
        ));
        assert!(matches!(
            decoded[1].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PlayStatusPacketStatus::PlayerSpawn
        ));
    }

    // ========== NetherNet (No Prefix) Tests ==========

    #[test]
    fn encode_decode_nethernet_format() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::LoginSuccess,
        });

        // NetherNet style: no 0xFE prefix
        let batch =
            encode_batch_multi(slice::from_ref(&packet), true, 7, 0, false).expect("encode");
        let mut buf = batch.clone();

        // Should NOT start with 0xFE
        assert_eq!(buf[0], BatchCompression::Deflate as u8);

        let decoded = decode_batch_no_prefix(&mut buf, &session, None).expect("decode");
        assert_eq!(decoded.len(), 1);
    }

    #[test]
    fn decode_nethernet_empty_returns_empty() {
        let session = test_session();
        let mut buf = Bytes::new();

        let decoded = decode_batch_no_prefix(&mut buf, &session, None).expect("decode empty");
        assert!(decoded.is_empty());
    }

    // ========== Decompression Guard Tests ==========

    #[test]
    fn decompression_guard_rejects_oversized() {
        let session = test_session();

        // Create packets that will decompress to more than 100 bytes
        let packets: Vec<_> = (0..10)
            .map(|_| {
                McpePacket::from(PlayStatusPacket {
                    status: PlayStatusPacketStatus::LoginSuccess,
                })
            })
            .collect();

        let batch = encode_batch_multi(&packets, true, 7, 0, true).expect("encode");
        let mut buf = batch.clone();

        // Very small max size
        let result = decode_batch(&mut buf, &session, true, Some(1));
        assert!(result.is_err());
    }

    // ========== Snappy Tests ==========

    #[test]
    fn decode_batch_accepts_snappy() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::LoginSuccess,
        });
        let raw_batch = encode_batch(&packet, false, 0, 0).expect("encode raw");
        let compressed = snap::raw::Encoder::new()
            .compress_vec(&raw_batch[1..])
            .expect("snappy compress");

        let mut framed = BytesMut::with_capacity(2 + compressed.len());
        framed.put_u8(BATCH_PACKET_ID);
        framed.put_u8(BatchCompression::Snappy as u8);
        framed.extend_from_slice(&compressed);

        let mut framed = framed.freeze();
        let decoded = decode_batch(&mut framed, &session, true, Some(1024)).expect("decode snappy");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            decoded[0].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PlayStatusPacketStatus::LoginSuccess
        ));
    }

    // ========== Raw Packet Tests ==========
    // Note: RawPacket creation requires going through decode_packets_raw,
    // which means we can test the roundtrip through encode -> decode.

    #[test]
    fn raw_batch_encode_decode_via_full_packet() {
        // Create a normal packet, encode it, then decode as raw
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::LoginSuccess,
        });

        // Encode as normal batch
        let batch = encode_batch(&packet, true, 7, 0).expect("encode");
        let mut buf = batch.clone();

        // Decode as raw
        let decoded_raw = decode_batch_raw(&mut buf, true, None).expect("decode raw");
        assert_eq!(decoded_raw.len(), 1);

        // Re-encode the raw packets
        let batch2 = encode_batch_raw(&decoded_raw, true, 7, 0, true).expect("encode raw");
        let mut buf2 = batch2.clone();

        // Decode as full packet to verify roundtrip
        let decoded = decode_batch(&mut buf2, &session, true, None).expect("decode full");
        assert_eq!(decoded.len(), 1);
        assert!(matches!(
            decoded[0].data,
            McpePacketData::PacketPlayStatus(ref s) if s.status == PlayStatusPacketStatus::LoginSuccess
        ));
    }

    #[test]
    fn raw_batch_uncompressed_roundtrip() {
        let session = test_session();
        let packet = McpePacket::from(PlayStatusPacket {
            status: PlayStatusPacketStatus::PlayerSpawn,
        });

        // Encode without compression
        let batch = encode_batch(&packet, false, 0, 0).expect("encode");
        let mut buf = batch.clone();

        // Decode as raw (no compression)
        let decoded_raw = decode_batch_raw(&mut buf, false, None).expect("decode raw");
        assert_eq!(decoded_raw.len(), 1);

        // Re-encode
        let batch2 = encode_batch_raw(&decoded_raw, false, 0, 0, true).expect("encode raw");
        let mut buf2 = batch2.clone();

        // Verify
        let decoded = decode_batch(&mut buf2, &session, false, None).expect("decode full");
        assert_eq!(decoded.len(), 1);
    }
}
