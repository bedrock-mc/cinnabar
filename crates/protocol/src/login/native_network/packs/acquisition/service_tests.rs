use super::super::{MAX_CDN_DOWNLOADS, wait_io};
use super::*;
use crate::login::native_network::PacketConnection;
use crate::packet_tap::{PacketDirection, PacketOutcome};
use crate::session_transport::{
    Inbound, SessionTransport,
    native::tests::service::{connection, request_service},
    tests::{RecordingTap, TapEvent},
};
use bedrock_client::packs::{PackEvent, PackOffer, PackTransferLimits};
use bedrock_protocol_1_26_51::{packets, types::PackInstanceId, wire};
use futures::{StreamExt, stream};
use resource_pack::test_support::archive_bytes;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
};

/// Advertises only an original synthetic archive to the real preload/cache owners.
fn offer(id: uuid::Uuid, size: usize) -> PackInfoData {
    let mut offer = PackInfoData::default();
    offer.pack_id_version.pack_uuid = id;
    offer.pack_id_version.pack_version.version = "1.0.0".into();
    offer.pack_size = wire::U64LE(size as u64);
    offer
}

#[tokio::test]
async fn pack_cdn_wait_keeps_service_live_and_retains_accepted_packet_prefix() {
    tokio::time::timeout(Duration::from_secs(10), cdn_wait(false))
        .await
        .expect("finite CDN/service fixture stalled");
}

#[tokio::test]
async fn public_pack_cdn_wait_keeps_service_live_and_retains_accepted_packet_prefix() {
    tokio::time::timeout(Duration::from_secs(10), cdn_wait(true))
        .await
        .expect("finite public CDN/service fixture stalled");
}

/// Holds a real HTTP body while the existing service boundary must complete its finite leases.
async fn cdn_wait(public: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let id = uuid::Uuid::from_u128(4);
    let bytes = archive_bytes(id, &[]);
    let mut offered = offer(id, bytes.len());
    offered.cdn_url = format!("http://{}/held-body", listener.local_addr().unwrap()).into();
    let endpoint = listener.local_addr().unwrap();
    let (started, ready) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let peer = async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await.unwrap());
            assert!(request.len() < 8192);
        }
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        started.send(()).unwrap();
        released.await.unwrap();
        socket.write_all(&bytes).await.unwrap();
    };
    let application = async {
        let (mut connection, incoming, service) = connection();
        let (_, healthy) = request_service(&service, false).await;
        connection.maintain().await;
        healthy.await.unwrap().unwrap();
        let packets = [1, 2].map(|time| {
            crate::Packet::new(packets::SetTime {
                time: wire::ZigZag32(time),
            })
        });
        for packet in &packets {
            incoming
                .send(crate::session_transport::batch_body(std::slice::from_ref(packet)).unwrap())
                .await
                .unwrap();
        }
        let cancelled = Cancellation::new();
        let client = if public {
            super::super::PUBLIC_CDN_ENDPOINT
                .scope(endpoint, async { public_http_client().unwrap() })
                .await
        } else {
            http_client().unwrap()
        };
        let preloads = stream::iter([preload(
            &offered,
            None,
            Some(client),
            Arc::clone(&cancelled.0),
        )])
        .buffer_unordered(MAX_CDN_DOWNLOADS)
        .collect::<Vec<_>>();
        let mut waiting = Box::pin(wait_io(&mut connection, preloads));
        tokio::select! {
            result = &mut waiting => { drop(result); panic!("HTTP body has not been supplied"); }
            result = ready => result.unwrap(),
        }
        // Real TCP/HTTP admission is complete; virtual time now makes lease failure deterministic.
        tokio::time::pause();
        for end in [false, false, true] {
            let (deadline, lease) = request_service(&service, end).await;
            tokio::select! {
                result = &mut waiting => { drop(result); panic!("external I/O must remain pending through service progress"); }
                result = tokio::time::timeout_at(deadline, lease) => {
                    result.expect("pending CDN body starved the existing service lease").unwrap().unwrap();
                }
            }
        }
        tokio::time::resume();
        release.send(()).unwrap();
        let preloads = waiting.await;
        assert_eq!(preloads.len(), 1);
        assert_eq!(preloads[0].archive.as_deref().unwrap(), bytes.as_slice());
        let tap = Arc::new(RecordingTap::default());
        let mut transport = SessionTransport::from_native(connection).with_tap(Some(tap.clone()));
        for (index, expected) in packets.iter().enumerate() {
            let Inbound::Packet(packet) = transport.recv().await.unwrap() else {
                panic!("accepted application data precedes the staged service terminal");
            };
            assert_eq!(packet.decode().unwrap(), *expected);
            if index == 0 {
                transport.reject_current(PacketOutcome::Skipped);
            }
        }
        assert!(matches!(transport.recv().await, Err(crate::SessionError(
            crate::codec::SessionFailure::NativeTransport(error)
        )) if error.kind() == io::ErrorKind::ConnectionAborted));
        let events = tap.events();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    TapEvent::Record(PacketDirection::Clientbound, _, _)
                ))
                .count(),
            packets.len()
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, TapEvent::Reject(_, _, PacketOutcome::Skipped, _)))
                .count(),
            1
        );
        transport.close().await.unwrap();
    };
    tokio::join!(peer, application);
}

#[tokio::test]
async fn pack_cache_finish_keeps_service_live_until_original_archive_handoff() {
    tokio::time::timeout(Duration::from_secs(10), cache_wait())
        .await
        .expect("finite cache/service fixture stalled");
}

/// Holds the existing write slot while the actual cache job and finish owner remain pending.
async fn cache_wait() {
    let root = tempfile::tempdir().unwrap();
    let cache = open_cache(Some(root.path())).await.unwrap();
    let cancelled = Cancellation::new();
    let id = uuid::Uuid::from_u128(5);
    let bytes = archive_bytes(id, &[]);
    let offered = offer(id, bytes.len());
    let mut negotiation = PackOffer::new(
        packets::ResourcePacksInfo {
            resource_packs: vec![offered.clone()],
            ..Default::default()
        },
        PackTransferLimits::default(),
    )
    .unwrap();
    negotiation.supply_preloaded(id, bytes.clone()).unwrap();
    let mut stores = Stores::new();
    let held = Arc::clone(&stores.slot).acquire_owned().await.unwrap();
    stores.register(&offered);
    let (mut negotiation, _) = negotiation.start().unwrap();
    stores.queue_completed(
        &Some(Arc::clone(&cache)),
        |id| negotiation.completed_archive(id),
        &cancelled.0,
    );
    assert_eq!(
        stores.jobs.len(),
        1,
        "the actual cache write must be retained"
    );
    let (mut connection, _incoming, service) = connection();
    let mut waiting = Box::pin(wait_io(&mut connection, stores.finish()));
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    tokio::time::pause();
    for _ in 0..2 {
        let (deadline, lease) = request_service(&service, false).await;
        tokio::select! {
            () = &mut waiting => panic!("held cache admission cannot complete"),
            result = tokio::time::timeout_at(deadline, lease) => {
                result.expect("pending cache finish starved the existing service lease").unwrap().unwrap();
            }
        }
    }
    tokio::time::resume();
    drop(held);
    waiting.await;
    let PackEvent::Finished(complete) = negotiation
        .receive(
            packets::ResourcePackStack {
                texture_pack_list: vec![PackInstanceId {
                    pack_id: id.to_string().into(),
                    version: "1.0.0".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }
            .into(),
        )
        .unwrap()
    else {
        panic!("finished cache work must release original archive ownership");
    };
    let (_, handoff) = complete.into_parts();
    assert_eq!(handoff.into_archives()[0].archive, bytes);
    let loaded = preload(&offered, Some(cache), None, Arc::clone(&cancelled.0)).await;
    assert!(loaded.cache_hit);
    assert_eq!(loaded.archive.unwrap(), bytes);
}
