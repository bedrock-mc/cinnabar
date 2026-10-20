use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::{PacketId, SetTime};
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixListener;

use super::PlaySession;
use crate::packet_tap::{PacketDirection, PacketOutcome};
use crate::session_transport::tests::{RecordingTap, TapEvent};
use crate::session_transport::{SessionTransport, batch_frame};
use crate::{Packet, WorldEvent};

/// Play reports the malformed packet that ended the session, with its body, and nothing for
/// the packets it accepted.
#[tokio::test]
async fn a_malformed_world_packet_is_reported_failed_with_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
    let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
    let (mut peer, _) = listener.accept().await.unwrap();
    let tap = Arc::new(RecordingTap::default());
    let transport = SessionTransport::new(reader, frames, &[]).with_tap(Some(tap.clone()));
    let mut session = PlaySession::new(transport, None, crate::login::startup::ready_fixture(1));

    let mut frame = batch_frame(&[Packet::new(SetTime {
        time: wire::ZigZag32(5),
    })])
    .unwrap()
    .to_vec();
    // A SetTime entry whose body is missing its time.
    frame.extend([1, PacketId::SetTime as u8]);
    peer.write_u32(frame.len() as u32).await.unwrap();
    peer.write_all(&frame).await.unwrap();

    assert!(matches!(
        session.recv_world_event(0).await.unwrap(),
        WorldEvent::SetTime(_)
    ));
    assert!(session.recv_world_event(0).await.is_err());
    assert_eq!(session.decode_error_count(), 1);
    let set_time = PacketId::SetTime as u32;
    let events = tap.events();
    assert!(matches!(
        &events[..2],
        [
            TapEvent::Record(PacketDirection::Clientbound, first, _),
            TapEvent::Record(PacketDirection::Clientbound, second, body),
        ] if *first == set_time && *second == set_time && body.is_empty()
    ));
    assert_eq!(
        events[2..],
        [TapEvent::Reject(
            1,
            set_time,
            PacketOutcome::Failed,
            Vec::new()
        )]
    );
}

/// Once accepted, the observer releases the batch even while the caller holds admission work.
#[tokio::test]
async fn accepted_packets_release_the_observers_batch_storage() {
    for (name, cache, world) in [
        ("ordinary", None, false),
        ("world", None, true),
        (
            "cached-world",
            Some(crate::ClientBlobCache::default()),
            true,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
        let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let tap = Arc::new(RecordingTap::default());
        let packet = crate::session_transport::tests::packet_bytes(&Packet::new(SetTime {
            time: wire::ZigZag32(5),
        }));
        let mut transport = SessionTransport::new(reader, frames, &[packet]).with_tap(Some(tap));
        let crate::session_transport::Inbound::Packet(raw) = transport.recv().await.unwrap() else {
            panic!("startup contains a packet");
        };
        let backing = raw.body().clone();
        transport.prepend(vec![raw]);
        let mut session =
            PlaySession::new(transport, cache, crate::login::startup::ready_fixture(1));
        if world {
            session
                .recv_world_event_mapped(0, |_| (), |_, _| ())
                .await
                .unwrap();
        } else {
            session.recv().await.unwrap();
        }
        assert!(backing.is_unique(), "{name} kept accepted packet storage");
    }
}

/// Cache validation reports the original rejected body before returning its recovery event.
#[tokio::test]
async fn cache_rejections_are_tapped_before_recovery() {
    use bedrock_protocol_1_26_51::{packets::ClientCacheMissResponse, types::MissingBlobData};
    let hash = crate::client_blob_hash(b"expected");
    let response = Packet::new(ClientCacheMissResponse {
        missing_blobs: vec![MissingBlobData {
            blob_id: wire::U64LE(hash),
            blob_data: Bytes::from_static(b"bad hash"),
        }],
    });
    let mut invalid_shape = super::tests::cached_level_chunk(vec![hash]);
    invalid_shape.cache_metadata.clear();
    let verified = Packet::new(ClientCacheMissResponse {
        missing_blobs: vec![MissingBlobData {
            blob_id: wire::U64LE(hash),
            blob_data: Bytes::from_static(b"expected"),
        }],
    });
    let mut missing = Vec::new();
    for (rejected, skipped) in [
        (response, true),
        (Packet::new(invalid_shape), true),
        (verified, false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
        let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let tap = Arc::new(RecordingTap::default());
        let wire = crate::session_transport::tests::packet_bytes(&rejected);
        let mut reader_body = wire::Reader::new(&wire);
        let id = reader_body.read_var_u32().unwrap();
        let body = wire.slice(reader_body.position()..);
        let transport = SessionTransport::new(reader, frames, &[wire]).with_tap(Some(tap.clone()));
        let mut session = PlaySession::new(
            transport,
            Some(crate::ClientBlobCache::default()),
            crate::login::startup::ready_fixture(1),
        );
        session
            .blob_cache
            .as_mut()
            .unwrap()
            .accept_cached_packet(Packet::new(super::tests::cached_level_chunk(vec![hash])))
            .unwrap();
        let event = session.recv_world_event(0).await.unwrap();
        if skipped {
            assert!(matches!(event, WorldEvent::ChunkResync(_)));
            if !tap.events().iter().any(|event| {
                matches!(event,
                    TapEvent::Reject(0, packet_id, PacketOutcome::Skipped, rejected_body)
                        if *packet_id == id && *rejected_body == body.as_ref()
                )
            }) {
                missing.push(id);
            }
        } else {
            assert!(matches!(event, WorldEvent::LevelChunk(_)));
            assert!(
                !tap.events()
                    .iter()
                    .any(|event| matches!(event, TapEvent::Reject(..)))
            );
        }
        assert_eq!(session.decode_error_count(), 0);
    }
    assert!(
        missing.is_empty(),
        "cache rejections lost packet evidence: {missing:?}"
    );
}

/// A rejected boundary keeps its evidence even when ingress continues to an ordinary event.
#[tokio::test]
async fn an_unusable_transfer_is_tapped_as_skipped() {
    use bedrock_protocol_1_26_51::packets::Transfer;
    for cache in [None, Some(crate::ClientBlobCache::default())] {
        let dir = tempfile::tempdir().unwrap();
        let listener = UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
        let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let tap = Arc::new(RecordingTap::default());
        let packet = crate::session_transport::tests::packet_bytes(&Packet::new(Transfer {
            server_address: "".into(),
            server_port: wire::U16LE(19132),
            ..Default::default()
        }));
        let mut body_reader = wire::Reader::new(&packet);
        let id = body_reader.read_var_u32().unwrap();
        let body = packet.slice(body_reader.position()..);
        let next = crate::session_transport::tests::packet_bytes(&Packet::new(SetTime {
            time: wire::ZigZag32(7),
        }));
        let transport =
            SessionTransport::new(reader, frames, &[packet, next]).with_tap(Some(tap.clone()));
        let mut session =
            PlaySession::new(transport, cache, crate::login::startup::ready_fixture(1));
        assert!(matches!(
            session.recv_world_event(0).await.unwrap(),
            WorldEvent::SetTime(_)
        ));
        assert_eq!(session.transfer_skip_count(), 1);
        assert!(
            tap.events().iter().any(|event| matches!(event,
                TapEvent::Reject(0, packet_id, PacketOutcome::Skipped, rejected_body)
                    if *packet_id == id && *rejected_body == body.as_ref()
            )),
            "transfer skip lost its packet evidence: {:?}",
            tap.events()
        );
    }
}
