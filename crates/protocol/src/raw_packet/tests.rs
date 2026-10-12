use bytes::BytesMut;

use super::*;

fn var_u32(out: &mut Vec<u8>, value: u32) {
    wire::Writer::new(out).write_var_u32(value).unwrap();
}

fn entry(packet_id: u32, from_sub: u32, to_sub: u32, body: &[u8]) -> Bytes {
    let mut payload = Vec::new();
    var_u32(&mut payload, packet_id | (from_sub << 10) | (to_sub << 12));
    payload.extend_from_slice(body);
    let mut frame = Vec::new();
    var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    Bytes::from(frame)
}

fn joined(entries: &[Bytes]) -> Bytes {
    let mut batch = BytesMut::new();
    for entry in entries {
        batch.extend_from_slice(entry);
    }
    batch.freeze()
}

#[test]
fn header_keeps_the_subclients_and_the_entry_bytes() {
    let frame = entry(0x02, 1, 2, &[0, 0, 0, 1]);
    let mut cursor = frame.clone();
    let raw = decode_packet_raw(&mut cursor).unwrap();
    assert_eq!(raw.id, PacketId::PlayStatus);
    assert_eq!((raw.header.from_subclient, raw.header.to_subclient), (1, 2));
    assert_eq!(raw.body().as_ref(), &[0, 0, 0, 1]);
    assert_eq!(raw.inner_frame(), &frame);
    assert!(cursor.is_empty());
}

/// Every ten-bit ID resolves exactly as the generated packet table names it.
#[test]
fn every_ten_bit_id_matches_the_packet_table() {
    for id in 0..=0x3ff_u32 {
        match (
            decode_packet_raw(&mut entry(id, 0, 0, &[])),
            PacketId::from_raw(id),
        ) {
            (Ok(raw), Some(expected)) => assert_eq!(raw.id, expected, "id={id}"),
            (Err(SessionError(SessionFailure::UnknownPacketId(unknown))), None) => {
                assert_eq!(unknown, id);
            }
            (actual, expected) => panic!("id={id}: {actual:?} vs {expected:?}"),
        }
    }
}

#[test]
fn decode_failure_names_the_packet_with_a_bounded_preview() {
    // Text declares a 127-byte message but carries 36 bytes, more than the 32-byte preview.
    let mut body = vec![0x00, 0x00, 0x00, 0x7f];
    body.extend(0u8..36);
    let raw = decode_packet_raw(&mut entry(0x09, 0, 0, &body)).unwrap();
    let error = raw.decode().expect_err("truncated Text");
    let message = error.to_string();
    let SessionError(SessionFailure::PacketDecode {
        packet_id,
        body_len,
        body_preview,
        ..
    }) = error
    else {
        panic!("expected a packet-aware decode error, got {message}")
    };
    assert_eq!(packet_id, PacketId::Text);
    assert_eq!(body_len, body.len());
    assert_eq!(body_preview, &body[..32]);
    assert!(message.contains("Text") && message.contains("body_len=40"));
}

#[test]
fn decodes_reject_trailing_body_bytes() {
    let raw = decode_packet_raw(&mut entry(0x02, 0, 0, &[0, 0, 0, 0, 0xaa])).unwrap();
    assert!(matches!(
        raw.decode(),
        Err(SessionError(SessionFailure::PacketTrailingBytes {
            remaining: 1,
            ..
        }))
    ));
}

#[test]
fn malformed_entries_are_refused() {
    assert!(matches!(
        decode_packet_raw(&mut Bytes::from_static(&[0x80])),
        Err(SessionError(SessionFailure::MalformedEntry(_)))
    ));
    let mut truncated = Vec::new();
    var_u32(&mut truncated, 100);
    truncated.extend_from_slice(&[1, 2, 3, 4, 5]);
    assert!(matches!(
        decode_packet_raw(&mut Bytes::from(truncated)),
        Err(SessionError(SessionFailure::TruncatedEntry {
            declared: 100,
            available: 5
        }))
    ));
}

#[test]
fn batch_bodies_split_in_order_within_their_bounds() {
    let packets = decode_batch_body(
        joined(&[entry(0x02, 0, 0, &[1]), entry(0x02, 1, 1, &[2])]),
        1024,
    )
    .unwrap();
    assert_eq!(packets.len(), 2);
    assert_eq!(packets[1].body().as_ref(), &[2]);
    assert!(decode_batch_body(Bytes::new(), 0).unwrap().is_empty());
    assert!(matches!(
        decode_batch_body(Bytes::from_static(&[0; 9]), 8),
        Err(SessionError(SessionFailure::BatchTooLarge {
            actual: 9,
            max: 8
        }))
    ));

    let one = entry(0x02, 0, 0, &[]);
    let at_limit = joined(&vec![one.clone(); MAX_RAW_BATCH_PACKETS]);
    assert_eq!(
        decode_batch_body(at_limit, usize::MAX).unwrap().len(),
        MAX_RAW_BATCH_PACKETS
    );
    let over_limit = joined(&vec![one; MAX_RAW_BATCH_PACKETS + 1]);
    assert!(matches!(
        decode_batch_body(over_limit, usize::MAX),
        Err(SessionError(SessionFailure::TooManyPackets { .. }))
    ));
}

#[test]
fn retention_bounded_packets_detach_from_large_siblings() {
    let batch = joined(&[
        entry(0x02, 0, 0, &[0x5a; 32]),
        entry(0x02, 0, 0, &vec![0x7b; 1024 * 1024]),
    ]);
    let packets = decode_batch_body(batch, usize::MAX).unwrap();
    let before = packets[0].body().as_ptr();
    let bounded = packets.into_iter().next().unwrap().into_retention_bounded();
    assert_ne!(bounded.body().as_ptr(), before);
    assert_eq!(bounded.body().as_ref(), &[0x5a; 32]);
    assert_eq!(bounded.backing_len, bounded.inner_frame().len());
}
