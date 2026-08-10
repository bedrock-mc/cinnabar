use super::*;

async fn assert_level_chunk_wire_failure(cache_enabled: bool, script: CachePlayScript) {
    let transport = ScriptTransport::new_with_options(
        CompressionMode::Deflate,
        SpawnOrder::RadiusThenSpawn,
        false,
        false,
        cache_enabled,
        script,
    );
    let connected = if cache_enabled {
        LoginSequence::connect_transport_with_blob_cache(
            transport,
            "RustClient",
            ClientBlobCache::default(),
        )
        .await
    } else {
        LoginSequence::connect_transport(transport, "RustClient").await
    };
    let (mut session, _) = connected.expect("scripted login");
    for _ in 0..3 {
        session.recv_world_event(0).await.expect("login prelude");
    }
    if cache_enabled {
        assert_eq!(
            session.recv_world_event(0).await.expect("pending traffic"),
            WorldEvent::SetTime(protocol::SetTimeEvent { time: 45_678 })
        );
        assert_eq!(session.blob_cache_stats().pending_transactions, 1);
        session.arm_blob_cache_reset_for_fast_transfer();
    }
    let error = session
        .recv_world_event_mapped(0, |_| (), |_, _| ())
        .await
        .expect_err("malformed LevelChunk must fail the session");
    let ProtocolError::Session(error) = error else {
        panic!("LevelChunk wire failure lost packet-aware session identity")
    };
    match (script, error) {
        (
            CachePlayScript::MalformedLevelChunk,
            jolyne::error::JolyneError::PacketDecode { packet_id, .. },
        ) => assert_eq!(packet_id, McpePacketName::LevelChunkPacket),
        (
            CachePlayScript::TrailingLevelChunk,
            jolyne::error::JolyneError::PacketTrailingBytes { packet_id, .. },
        ) => assert_eq!(packet_id, McpePacketName::LevelChunkPacket),
        (_, other) => panic!("unexpected LevelChunk failure: {other:?}"),
    }
    assert_eq!(session.decode_error_count(), 1);
    if cache_enabled {
        // Fatal borrowed-decode recovery clears both the pending transaction
        // and the armed fast-transfer barrier before later traffic resumes.
        assert_eq!(session.blob_cache_stats().pending_transactions, 0);
        let (event, payload) = session
            .recv_world_event_mapped(0, |_| None, |event, payload| Some((event, payload)))
            .await
            .expect("valid post-failure LevelChunk")
            .expect("valid LevelChunk byte ingress");
        assert_eq!((event.x, event.z), (7, -8));
        assert_eq!(payload.as_ref(), vec![0x4d; 4096]);
        assert_eq!(session.blob_cache_stats().pending_transactions, 0);
    } else {
        assert_eq!(session.blob_cache_stats().pending_transactions, 0);
    }
}

#[tokio::test]
async fn borrowed_level_chunk_wire_failures_keep_identity_with_and_without_resolver() {
    for cache_enabled in [false, true] {
        assert_level_chunk_wire_failure(cache_enabled, CachePlayScript::MalformedLevelChunk).await;
        assert_level_chunk_wire_failure(cache_enabled, CachePlayScript::TrailingLevelChunk).await;
    }
}
