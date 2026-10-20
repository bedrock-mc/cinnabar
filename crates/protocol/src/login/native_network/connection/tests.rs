use super::*;

/// Uses the real generated encoder while leaving the original header/body bytes intact.
fn packet_bytes(packet: &Packet) -> Bytes {
    let batch = crate::codec::encode(packet).unwrap();
    let body = batch.slice(1..);
    let mut reader = wire::Reader::from_shared(&body);
    reader.read_var_u32().unwrap();
    body.slice(reader.position()..)
}

#[test]
fn early_native_packets_keep_original_subclient_headers_and_fifo() {
    let mut first = Packet::new(packets::SetTime {
        time: wire::ZigZag32(1),
    });
    first.header.from_subclient = 1;
    first.header.to_subclient = 2;
    let second = Packet::new(packets::SetTime {
        time: wire::ZigZag32(2),
    });
    let mut deferred = DeferredPackets::default();
    for packet in [&first, &second] {
        retain_early(&mut deferred, packet_bytes(packet)).unwrap();
    }
    let retained = deferred
        .into_packets()
        .into_iter()
        .map(|packet| packet.decode().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(retained, [first, second]);
}

#[test]
fn early_native_packets_share_the_existing_aggregate_startup_byte_bound() {
    let chunk = Packet::new(packets::LevelChunk {
        serialized_chunk_data: vec![0; MAX_DEFERRED_PACKET_BYTES / 2].into(),
        ..Default::default()
    });
    let bytes = packet_bytes(&chunk);
    let mut deferred = DeferredPackets::default();
    retain_early(&mut deferred, bytes.clone()).unwrap();
    assert!(matches!(
        retain_early(&mut deferred, bytes),
        Err(crate::ProtocolError::Session(crate::SessionError(
            crate::codec::SessionFailure::BatchTooLarge {
                max: MAX_DEFERRED_PACKET_BYTES,
                ..
            }
        )))
    ));
    let retained = deferred.into_packets();
    assert_eq!(retained.len(), 1);
    assert_eq!(
        retained.into_iter().next().unwrap().decode().unwrap(),
        chunk
    );
}
