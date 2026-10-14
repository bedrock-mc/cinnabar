use super::*;

const HEADER: CaptureHeader = CaptureHeader {
    protocol_version: crate::PROTOCOL_VERSION,
    started_unix_ms: 1_760_000_000_123,
};

fn packet(
    direction: PacketDirection,
    packet_id: u32,
    elapsed_us: u64,
    body: &[u8],
) -> CaptureRecord {
    CaptureRecord::Packet {
        direction,
        packet_id,
        elapsed_us,
        body: Bytes::copy_from_slice(body),
    }
}

fn encode(limit: u64, records: &[CaptureRecord]) -> (Vec<u8>, Vec<bool>) {
    let mut writer = CaptureWriter::with_limit(Vec::new(), HEADER, limit).unwrap();
    let accepted = records
        .iter()
        .map(|record| writer.write(record).unwrap())
        .collect();
    let written = writer.written();
    let bytes = writer.into_inner();
    assert_eq!(written, bytes.len() as u64);
    (bytes, accepted)
}

#[test]
fn records_round_trip_with_their_header_and_little_endian_layout() {
    let records = [
        packet(PacketDirection::Clientbound, 11, 0, &[1, 2, 3]),
        packet(PacketDirection::Serverbound, 0x90, 1_500, &[]),
        CaptureRecord::Gap {
            elapsed_us: 2_000,
            dropped: 7,
        },
        packet(PacketDirection::Clientbound, 0x3ff, u64::MAX, &[0xff; 300]),
    ];
    let (bytes, accepted) = encode(MAX_CAPTURE_BYTES, &records);
    assert_eq!(accepted, [true; 4]);
    assert_eq!(&bytes[..8], b"CNBRCAP2");
    assert_eq!(&bytes[8..10], &2_u16.to_le_bytes());
    assert_eq!(&bytes[10..14], &crate::PROTOCOL_VERSION.to_le_bytes());
    assert_eq!(&bytes[14..22], &HEADER.started_unix_ms.to_le_bytes());
    let mut first = vec![0_u8];
    first.extend(11_u32.to_le_bytes());
    first.extend(0_u64.to_le_bytes());
    first.extend(3_u32.to_le_bytes());
    first.extend([1, 2, 3]);
    assert_eq!(&bytes[22..42], first.as_slice());
    let expected_len =
        CAPTURE_HEADER_BYTES + records.iter().map(CaptureRecord::encoded_len).sum::<u64>();
    assert_eq!(bytes.len() as u64, expected_len);
    assert_eq!(
        read_capture(Bytes::from(bytes)).unwrap(),
        (HEADER, records.to_vec())
    );
}

#[test]
fn gap_records_count_dropped_packets_in_stream_order() {
    let records = [
        packet(PacketDirection::Clientbound, 11, 10, &[1]),
        CaptureRecord::Gap {
            elapsed_us: 20,
            dropped: u64::MAX,
        },
        packet(PacketDirection::Clientbound, 9, 30, &[2]),
    ];
    let (bytes, _) = encode(MAX_CAPTURE_BYTES, &records);
    let (_, read) = read_capture(Bytes::from(bytes)).unwrap();
    assert_eq!(read, records);
    assert!(matches!(
        read[1],
        CaptureRecord::Gap {
            dropped: u64::MAX,
            ..
        }
    ));
}

#[test]
fn size_limit_ends_the_capture_with_one_final_marker_and_never_exceeds_it() {
    let body = [7_u8; 10];
    let one = RECORD_HEADER_BYTES + body.len() as u64;
    // Room for two packets and the final marker, but not a third packet.
    let limit = CAPTURE_HEADER_BYTES + 2 * one + RECORD_HEADER_BYTES + one - 1;
    let records: Vec<_> = (0..5)
        .map(|index| packet(PacketDirection::Clientbound, 9, index * 100, &body))
        .collect();
    let (bytes, accepted) = encode(limit, &records);
    assert_eq!(accepted, [true, true, false, false, false]);
    assert!(bytes.len() as u64 <= limit);
    let (_, read) = read_capture(Bytes::from(bytes)).unwrap();
    assert_eq!(
        read,
        [
            records[0].clone(),
            records[1].clone(),
            CaptureRecord::Truncated { elapsed_us: 200 }
        ]
    );
    let exact = CAPTURE_HEADER_BYTES + 3 * one + RECORD_HEADER_BYTES;
    assert_eq!(encode(exact, &records).1, [true, true, true, false, false]);
    assert!(CaptureWriter::with_limit(Vec::new(), HEADER, CAPTURE_HEADER_BYTES).is_err());
}

#[test]
fn reader_rejects_truncation_unknown_kinds_and_records_after_the_final_marker() {
    let (bytes, _) = encode(
        MAX_CAPTURE_BYTES,
        &[packet(PacketDirection::Clientbound, 11, 0, &[1, 2, 3])],
    );
    for cut in 0..bytes.len() {
        if cut == CAPTURE_HEADER_BYTES as usize {
            assert_eq!(
                read_capture(Bytes::copy_from_slice(&bytes[..cut])).map(|(_, records)| records),
                Ok(Vec::new())
            );
            continue;
        }
        assert!(read_capture(Bytes::copy_from_slice(&bytes[..cut])).is_err());
    }
    let mut version = bytes.clone();
    version[8] = 3;
    assert_eq!(
        read_capture(Bytes::from(version)),
        Err(CaptureError::Version(3))
    );
    let mut kind = bytes.clone();
    kind[CAPTURE_HEADER_BYTES as usize] = 9;
    assert_eq!(
        read_capture(Bytes::from(kind)),
        Err(CaptureError::Kind { record: 0, kind: 9 })
    );
    let mut after_end = Vec::new();
    let mut writer = CaptureWriter::with_limit(
        &mut after_end,
        HEADER,
        CAPTURE_HEADER_BYTES + RECORD_HEADER_BYTES,
    )
    .unwrap();
    assert!(
        !writer
            .write(&packet(PacketDirection::Clientbound, 11, 5, &[1]))
            .unwrap()
    );
    after_end.extend_from_slice(&bytes[CAPTURE_HEADER_BYTES as usize..]);
    assert_eq!(
        read_capture(Bytes::from(after_end)),
        Err(CaptureError::AfterEnd { record: 1 })
    );
}

#[test]
fn v1_streams_are_not_mistaken_for_v2() {
    let v1 = [11_u32.to_le_bytes(), 0_u32.to_le_bytes()].concat();
    assert!(!is_capture_v2(&v1));
    assert_eq!(read_capture(Bytes::from(v1)), Err(CaptureError::Magic));
}
