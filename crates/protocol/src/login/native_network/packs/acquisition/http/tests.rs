use super::*;
use crate::login::native_network::packs::{PUBLIC_CDN_ENDPOINT, acquisition};
use bedrock_protocol_1_26_51::{types::PackInfoData, wire};
use resource_pack::test_support::archive_bytes;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// Polls the actual listener after the denied request has completed and dropped its connection work.
async fn unaccepted(listener: &TcpListener) {
    let accepting = listener.accept();
    tokio::pin!(accepting);
    assert!(futures::poll!(accepting.as_mut()).is_pending());
}

#[test]
fn public_urls_reject_private_literals_and_nonrequest_metadata() {
    let policy = Public { fixture: None };
    for value in [
        "http://127.0.0.1/pack",
        "http://10.0.0.1/pack",
        "http://[::1]/pack",
        "http://[::ffff:127.0.0.1]/pack",
        "http://[fe80::1%25en0]/pack",
        "http://1.1.1.1:0/pack",
        "http://user@1.1.1.1/pack",
        "http://1.1.1.1/pack#token",
        "file:///pack",
    ] {
        assert!(match Url::parse(value) {
            Ok(url) => policy.admit(&url).is_err(),
            Err(_) => true,
        });
    }
    assert!(
        policy
            .admit(&Url::parse("https://1.1.1.1/pack?token=opaque").unwrap())
            .is_ok()
    );
}

#[test]
fn actual_candidate_policy_preserves_admitted_order_and_bounds_the_original_prefix() {
    let first: SocketAddr = "1.1.1.1:0".parse().unwrap();
    let second: SocketAddr = "[2606:4700:4700::1111]:0".parse().unwrap();
    let denied: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let scoped = SocketAddr::V6(std::net::SocketAddrV6::new(
        match second.ip() {
            std::net::IpAddr::V6(ip) => ip,
            _ => unreachable!(),
        },
        0,
        0,
        1,
    ));
    assert_eq!(
        public_candidates([denied, first, scoped, second].into_iter()),
        [first, second]
    );
    assert!(
        public_candidates(std::iter::repeat_n(denied, GATEWAY_EGRESS_CANDIDATES).chain([first]))
            .is_empty()
    );
}

#[tokio::test]
async fn public_literal_and_actual_localhost_dns_do_not_connect_to_private_listener() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let client = public_http_client().unwrap();
        assert!(client.get(&format!("http://{endpoint}/literal")).is_err());
        assert!(
            client
                .get(&format!("http://localhost:{}/dns", endpoint.port()))
                .unwrap()
                .send()
                .await
                .is_err()
        );
        unaccepted(&listener).await;
    })
    .await
    .expect("bounded public DNS refusal stalled");
}

/// Serves only the original owned endpoint; a redirect target remains an independent denied socket.
async fn response(listener: TcpListener, response: Vec<u8>) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        header.push(socket.read_u8().await.unwrap());
        assert!(header.len() < 8192);
    }
    if let Err(error) = socket.write_all(&response).await {
        assert!(matches!(
            error.kind(),
            io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
        ));
    }
}

#[tokio::test]
async fn one_join_endpoint_exception_does_not_authorize_a_private_redirect_or_another_client() {
    tokio::time::timeout(Duration::from_secs(10), async {
        for dns_target in [false, true] {
            let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = origin.local_addr().unwrap();
            let url = format!("http://{endpoint}/signed?token=opaque");
            let client = PUBLIC_CDN_ENDPOINT.scope(endpoint, async { public_http_client().unwrap() }).await;
            assert!(public_http_client().unwrap().get(&url).is_err());
            let target_url = if dns_target {
                format!("http://localhost:{}/denied", target.local_addr().unwrap().port())
            } else {
                format!("http://{}/denied", target.local_addr().unwrap())
            };
            let reply = format!("HTTP/1.1 302 Found\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes();
            let (result, ()) = tokio::join!(client.get(&url).unwrap().send(), response(origin, reply));
            assert!(result.is_err(), "the captured exception cannot authorize a different redirect tuple or DNS host");
            unaccepted(&target).await;
        }
    }).await.expect("bounded redirect refusal stalled");
}

#[tokio::test]
async fn guarded_preload_rejects_bad_identity_and_size_before_protocol_supply() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let id = uuid::Uuid::from_u128(701);
        let bytes = archive_bytes(uuid::Uuid::from_u128(702), &[]);
        let cancelled = acquisition::Cancellation::new();
        for advertised in [bytes.len(), 1] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = listener.local_addr().unwrap();
            let client = PUBLIC_CDN_ENDPOINT
                .scope(endpoint, async { public_http_client().unwrap() })
                .await;
            let mut offer = PackInfoData::default();
            offer.pack_id_version.pack_uuid = id;
            offer.pack_id_version.pack_version.version = "1.0.0".into();
            offer.pack_size = wire::U64LE(advertised as u64);
            offer.cdn_url = format!("http://{endpoint}/pack").into();
            let mut reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .into_bytes();
            reply.extend_from_slice(&bytes);
            let (result, ()) = tokio::join!(
                acquisition::preload(&offer, None, Some(client), Arc::clone(&cancelled.0)),
                response(listener, reply)
            );
            assert!(
                result.archive.is_none(),
                "invalid public archive must retain original game-chunk fallback"
            );
            assert!(!result.cache_hit);
        }
    })
    .await
    .expect("bounded invalid archive fixture stalled");
}
