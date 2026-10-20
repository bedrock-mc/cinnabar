//! Actual Quinn/h3 codec peers exercise the gateway's original admission and stream owners.

mod packs;
pub(crate) mod peer;

use super::*;
use bridge::relay_control::{Capability, Event, Hello, Request as ControlRequest};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use peer::{Fixture, Peer};
use protocol::session_wire::{self, CoreMessage, SessionDisconnect};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::codec::Framed;

type Client = Framed<tokio::io::Join<quinn::RecvStream, quinn::SendStream>, bridge::BridgeCodec>;

/// Sends only shared schema values; WT play authority comes from its actual controller, not resume.
fn hello(channel: &str, generation: u64, join: u64) -> Bytes {
    Bytes::from(
        serde_json::to_vec(&Hello {
            version: RELAY_VERSION,
            channel: channel.into(),
            channels: vec![RELAY_CHANNEL_CONTROL.into(), RELAY_CHANNEL_PLAY.into()],
            resume: String::new(),
            generation,
            join,
        })
        .unwrap(),
    )
}

/// Reads an actual bounded control event from the retained same QUIC stream.
pub(crate) async fn event(client: &mut Client) -> Event {
    serde_json::from_slice(&client.next().await.unwrap().unwrap()).unwrap()
}

/// Establishes the actual account/controller capability; no manually constructed account or Ready.
pub(crate) async fn control(peer: &Peer) -> (Client, Capability) {
    let stream = peer.stream().await;
    let mut control = Framed::new(stream, bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES));
    control
        .send(hello(RELAY_CHANNEL_CONTROL, 0, 0))
        .await
        .unwrap();
    let Event::Welcome { capability } = event(&mut control).await else {
        panic!("actual Welcome")
    };
    assert!(matches!(event(&mut control).await, Event::Account { .. }));
    (control, capability)
}

/// A completed play cannot discard a taken replacement stream's partial WT prefix.
#[tokio::test]
async fn paired_control_survives_sequential_plays_and_a_retained_partial_prefix() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut fixture = Fixture::with_prefix_credit().await;
        let mut peer = fixture.peer(RELAY_TRANSPORT_PATH, None).await;
        assert_eq!(peer.status().await, StatusCode::OK);
        let (mut control, capability) = control(&peer).await;
        let mut first = Framed::new(
            peer.stream().await,
            bridge::BridgeCodec::with_max(bridge::MAX_FRAME_LEN),
        );
        first
            .send(hello(RELAY_CHANNEL_PLAY, capability.generation, 1))
            .await
            .unwrap();
        // One-byte receive credit cannot advance until the original h3 reader consumes it.
        let (mut next_write, next_read) = peer.connection.open_bi().await.unwrap();
        let prefix = peer.prefix();
        assert!(prefix.len() > 2);
        let incomplete = prefix.len() - 1;
        assert_eq!(next_write.write(&prefix[..incomplete]).await.unwrap(), 1);
        {
            let credit = next_write.write_all(&prefix[1..incomplete]);
            tokio::pin!(credit);
            assert!(
                futures::poll!(&mut credit).is_pending(),
                "the actual second prefix byte must wait for server receive credit"
            );
            credit.await.unwrap();
        }
        // h3 has consumed the first byte, but the final session-ID byte stays withheld.
        first.send(newer_connect()).await.unwrap();
        let expected =
            session_wire::encode_core_message(&CoreMessage::Disconnect(SessionDisconnect {
                reason: 0,
                message: DISCONNECT_KEY_OUTDATED_SERVER.into(),
                filtered_message: String::new(),
                hide_screen: false,
            }))
            .unwrap();
        assert_eq!(first.next().await.unwrap().unwrap(), expected);
        assert!(
            first.next().await.is_none(),
            "actual refused play sends FIN after its complete terminal"
        );
        next_write.write_all(&prefix[incomplete..]).await.unwrap();
        let mut second = Framed::new(
            tokio::io::join(next_read, next_write),
            bridge::BridgeCodec::with_max(bridge::MAX_FRAME_LEN),
        );
        second
            .send(hello(RELAY_CHANNEL_PLAY, capability.generation, 2))
            .await
            .unwrap();
        second.send(newer_connect()).await.unwrap();
        assert_eq!(second.next().await.unwrap().unwrap(), expected);
        assert!(second.next().await.is_none());
        control
            .send(Bytes::from(
                serde_json::to_vec(&ControlRequest::SignOut).unwrap(),
            ))
            .await
            .unwrap();
        loop {
            match event(&mut control).await {
                Event::Account { .. } => break,
                Event::Connect { connect } => assert_eq!(connect.stage, bridge::ConnectStage::Idle),
                other => panic!("original control changed: {other:?}"),
            }
        }
        assert_eq!(
            crate::registry::tests::retained_counts(&fixture.registry),
            (1, 1),
            "one QUIC connection owns both plays and its one account"
        );
        drop(first);
        drop(second);
        drop(control);
        peer.close();
        fixture.finish().await;
    })
    .await
    .expect("actual paired prefix/plays/control stalled");
}

/// Extra CONNECT and client application uni are refused without replacing the original controller.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_session_refuses_extra_connect_and_client_uni() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut fixture = Fixture::new(None).await;
        let mut peer = fixture.peer(RELAY_TRANSPORT_PATH, None).await;
        assert_eq!(peer.status().await, StatusCode::OK);
        let (mut control, _) = control(&peer).await;
        let mut extra = peer.request(RELAY_TRANSPORT_PATH, None).await;
        assert_eq!(
            peer.response(&mut extra).await,
            StatusCode::TOO_MANY_REQUESTS
        );
        let mut uni = peer.connection.open_uni().await.unwrap();
        use h3::stream::{UniStreamHeader, WriteBuf};
        let mut prefix = WriteBuf::<Bytes>::from(UniStreamHeader::WebTransportUni(peer.session));
        uni.write_all_buf(&mut prefix).await.unwrap();
        assert_eq!(uni.stopped().await.unwrap(), Some(0_u32.into()));
        control
            .send(Bytes::from(
                serde_json::to_vec(&ControlRequest::SignOut).unwrap(),
            ))
            .await
            .unwrap();
        assert!(matches!(event(&mut control).await, Event::Account { .. }));
        assert_eq!(
            crate::registry::tests::retained_counts(&fixture.registry),
            (1, 1)
        );
        drop(control);
        drop(extra);
        peer.close();
        fixture.finish().await;
    })
    .await
    .expect("actual WT refusal fixture stalled");
}

/// Actual CONNECT DATA/FIN must remain observable before the dependency h3 owner closes QUIC.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_eof_preserves_connect_close_output_before_physical_retirement() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut fixture = Fixture::new(None).await;
        let mut peer = fixture.peer(RELAY_TRANSPORT_PATH, None).await;
        assert_eq!(peer.status().await, StatusCode::OK);
        let (mut control, _) = control(&peer).await;
        control.close().await.unwrap();
        assert!(
            peer.connect_end().await > 0,
            "original close capsule DATA was discarded before FIN"
        );
        assert!(
            peer.connection.close_reason().is_none(),
            "h3 owner closed QUIC before explicit bounded retirement"
        );
        peer.close();
        drop(control);
        fixture.finish().await;
    })
    .await
    .expect("actual CONNECT close/owner handoff stalled");
}

/// Reuses the original first-Origin predicate and generated path before any success/account claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_connect_refuses_origin_and_path_before_account_attachment() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut fixture = Fixture::new(None).await;
        for (path, origin, status) in [
            (
                RELAY_TRANSPORT_PATH,
                Some("https://foreign.example"),
                StatusCode::FORBIDDEN,
            ),
            ("/unknown", None, StatusCode::NOT_FOUND),
        ] {
            let mut peer = fixture.peer(path, origin).await;
            assert_eq!(peer.status().await, status);
            peer.close();
            test_time::eventually(
                "refused QUIC handler releases admission without an account",
                || crate::registry::tests::retained_counts(&fixture.registry) == (0, 0),
            );
        }
        fixture.finish().await;
    })
    .await
    .expect("actual WT request refusal stalled");
}

/// Original TCP pre-TLS owners consume the SAME per-IP capacity before a QUIC TLS handshake.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_transports_share_pre_tls_admission() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut fixture = Fixture::new(None).await;
        let mut held = Vec::new();
        for _ in 0..RELAY_MAX_PER_IP {
            held.push(tokio::net::TcpStream::connect(fixture.tcp).await.unwrap());
        }
        test_time::eventually("same pre-TLS TCP admission fills paired capacity", || {
            crate::registry::tests::retained_counts(&fixture.registry).0 == RELAY_MAX_PER_IP
        });
        assert!(
            fixture.connect().await.is_err(),
            "QUIC TLS bypassed the original TCP/IP permit"
        );
        assert_eq!(
            crate::registry::tests::retained_counts(&fixture.registry),
            (RELAY_MAX_PER_IP, 0)
        );
        fixture.finish().await;
        for mut stream in held {
            assert!(matches!(stream.read(&mut [0]).await, Ok(0) | Err(_)));
        }
    })
    .await
    .expect("actual paired pre-TLS capacity stalled");
}

/// Uses current native metadata and changes only its newer protocol, before any target/socket work.
fn newer_connect() -> Bytes {
    let mut claims = protocol::login_client_data(
        "Fixture",
        protocol::ClientRandomId::new(1).unwrap(),
        None,
        &protocol::LoginSettings::default(),
    );
    claims["ServerAddress"] =
        std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, DEFAULT_BEDROCK_PORT))
            .to_string()
            .into();
    let raw = serde_json::value::to_raw_value(&claims).unwrap();
    assert!(
        protocol::NativeDeviceProfile::fresh()
            .validate_relay(
                protocol::RawClientData::from_raw(&raw).unwrap(),
                protocol::PROTOCOL_VERSION,
            )
            .is_ok(),
        "current metadata must validate before only the newer protocol refuses"
    );
    session_wire::encode_connect(&session_wire::ConnectRequest {
        protocol: protocol::PROTOCOL_VERSION.checked_add(1).unwrap(),
        join_profile_id: None,
        target: None,
        client_cache: true,
        client_data: claims,
    })
    .unwrap()
}
