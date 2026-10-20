use super::*;
use bedrock_client::packs::{PackEvent, PackOffer, PackTransferLimits};
use bedrock_protocol_1_26_51::{types::PackInfoData, wire};
use resource_pack::test_support::archive_bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Advertises a synthetic archive without production server identities.
fn offer(id: uuid::Uuid, size: usize) -> PackInfoData {
    let mut offer = PackInfoData::default();
    offer.pack_id_version.pack_uuid = id;
    offer.pack_id_version.pack_version.version = "1.0.0".into();
    offer.pack_size = wire::U64LE(size as u64);
    offer
}

/// The active Go fallback and reusable Rust owner share the same request-window policy.
#[test]
fn reusable_pack_policy_matches_the_existing_go_baseline() {
    let window: u32 = include_str!("../../../../../../core/proxy/resource_pack_window.txt")
        .trim()
        .parse()
        .unwrap();
    assert_eq!(window, bedrock_client::packs::DEFAULT_REQUEST_WINDOW);
    let baseline = include_str!("../../../../../../core/go.mod")
        .lines()
        .find(|line| {
            line.split_whitespace()
                .take(2)
                .eq(["github.com/sandertv/gophertunnel", "=>"])
        })
        .expect("active Gophertunnel behavior baseline")
        .split_whitespace()
        .last()
        .unwrap();
    assert_eq!(baseline, bedrock_client::packs::behavioral_baseline());
}

/// Serves one HTTP response after a complete bounded request header.
async fn serve(listener: tokio::net::TcpListener, response: Vec<u8>) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        request.push(socket.read_u8().await.unwrap());
        assert!(request.len() < 8192);
    }
    socket.write_all(&response).await.unwrap();
}

#[tokio::test]
async fn cdn_accepts_original_archive_and_cache_hit_avoids_http_while_bad_body_falls_back() {
    let root = tempfile::tempdir().unwrap();
    let cache = acquisition::open_cache(Some(root.path())).await.unwrap();
    let cancelled = acquisition::Cancellation::new();
    let id = uuid::Uuid::from_u128(1);
    let bytes = archive_bytes(id, &[]);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut offered = offer(id, bytes.len());
    offered.cdn_url = format!("http://{}/pack", listener.local_addr().unwrap()).into();
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    response.extend_from_slice(&bytes);
    let client = acquisition::http_client().unwrap();
    let (download, ()) = tokio::join!(
        acquisition::preload(
            &offered,
            Some(Arc::clone(&cache)),
            Some(client.clone()),
            Arc::clone(&cancelled.0)
        ),
        serve(listener, response)
    );
    assert_eq!(
        download
            .archive
            .as_deref()
            .expect("completed CDN acquisition"),
        &bytes
    );
    let mut negotiation = PackOffer::new(
        packets::ResourcePacksInfo {
            resource_packs: vec![offered.clone()],
            ..Default::default()
        },
        PackTransferLimits::default(),
    )
    .unwrap();
    negotiation
        .supply_preloaded(id, download.archive.unwrap())
        .unwrap();
    let mut stores = acquisition::Stores::new();
    stores.register(&offered);
    let (mut negotiation, _) = negotiation.start().unwrap();
    stores.queue_completed(
        &Some(Arc::clone(&cache)),
        |id| negotiation.completed_archive(id),
        &cancelled.0,
    );
    stores.finish().await;
    let PackEvent::Finished(complete) = negotiation
        .receive(
            packets::ResourcePackStack {
                texture_pack_list: vec![bedrock_protocol_1_26_51::types::PackInstanceId {
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
        panic!("cache stores must release final archive ownership");
    };
    let (_, handoff) = complete.into_parts();
    assert_eq!(handoff.into_archives()[0].archive, bytes);
    // The fixture listener is closed; only the persistent cache can satisfy this retry.
    let hit = acquisition::preload(
        &offered,
        Some(cache),
        Some(client.clone()),
        Arc::clone(&cancelled.0),
    )
    .await;
    assert_eq!(
        hit.archive.as_deref().expect("completed cache acquisition"),
        &bytes
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut offered = offer(uuid::Uuid::from_u128(2), 4);
    offered.cdn_url = format!("http://{}/oversized", listener.local_addr().unwrap()).into();
    let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\n12345\r\n0\r\n\r\n".to_vec();
    let (fallback, ()) = tokio::join!(
        acquisition::preload(&offered, None, Some(client), Arc::clone(&cancelled.0)),
        serve(listener, response)
    );
    assert!(fallback.archive.is_none());
}

#[tokio::test]
async fn dropping_cdn_acquisition_closes_the_pending_body_without_cache_objects() {
    let root = tempfile::tempdir().unwrap();
    let cache = acquisition::open_cache(Some(root.path())).await.unwrap();
    let cancelled = acquisition::Cancellation::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut offered = offer(uuid::Uuid::from_u128(3), 1024);
    offered.cdn_url = format!("http://{}/pending", listener.local_addr().unwrap()).into();
    let (started, ready) = tokio::sync::oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(socket.read_u8().await.unwrap());
            assert!(header.len() < 8192);
        }
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\n")
            .await
            .unwrap();
        started.send(()).unwrap();
        match socket.read(&mut [0_u8; 1]).await {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                ) => {}
            result => panic!("canceled download must close its pending body: {result:?}"),
        }
    });
    let mut preload = Box::pin(acquisition::preload(
        &offered,
        Some(cache),
        Some(acquisition::http_client().unwrap()),
        Arc::clone(&cancelled.0),
    ));
    tokio::select! {
        result = &mut preload => { drop(result); panic!("the fixture has not supplied its body"); }
        result = ready => result.unwrap(),
    }
    drop(preload);
    tokio::time::timeout(std::time::Duration::from_secs(5), peer)
        .await
        .expect("canceled download retained its peer socket")
        .unwrap();
    assert!(
        std::fs::read_dir(root.path().join("native-v1"))
            .unwrap()
            .all(|entry| entry.unwrap().file_name() == ".packcache.lock")
    );
}
