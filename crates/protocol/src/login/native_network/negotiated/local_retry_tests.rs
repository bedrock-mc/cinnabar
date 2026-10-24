use super::*;
use crate::login::native_network::{
    tests::provider,
    transport::early_spawn_tests::{FixtureRoute, Route, fixture_route, receive, send},
};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, CompressionAlgorithm, CompressionSettings,
};
use bedrock_protocol_1_26_51::{
    enums::ConnectionDisconnectFailReason, packets, types::DisconnectMessages, wire,
};
use futures::FutureExt;
use nethernet_tokio::AcceptedSession;
use std::time::{Duration, UNIX_EPOCH};

/// Completes the real settings exchange and retains the actual Login request.
async fn login(
    peer: &mut AcceptedSession,
) -> (BatchCodec<bedrock_net::Negotiated>, packets::Login) {
    let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
    let settings = receive(peer, &codec).await;
    assert!(matches!(
        &*settings[0].data,
        packets::Packet::RequestNetworkSettings(_)
    ));
    send(
        peer,
        &codec,
        &[crate::Packet::new(packets::NetworkSettings {
            compression_threshold: wire::U16LE(u16::MAX),
            ..Default::default()
        })],
    )
    .await;
    let codec = codec.negotiate(CompressionSettings {
        algorithm: CompressionAlgorithm::Deflate,
        threshold: u16::MAX,
    });
    let packets = receive(peer, &codec).await;
    let packets::Packet::Login(login) = &*packets[0].data else {
        panic!("expected the actual native Login");
    };
    (codec, login.clone())
}

/// Uses identical text for distinct typed reasons, so text alone cannot authorize a retry.
async fn reject(
    peer: &AcceptedSession,
    codec: &BatchCodec<bedrock_net::Negotiated>,
    reason: ConnectionDisconnectFailReason,
) {
    send(
        peer,
        codec,
        &[crate::Packet::new(packets::Disconnect {
            reason,
            messages: DisconnectMessages::DisconnectPacketMessages {
                message: "Hostwithoutkeys".into(),
                filtered_message: "Hostwithoutkeys".into(),
            },
        })],
    )
    .await;
}

/// Drives one original HTTP fixture; no task, real account or second deadline is introduced.
struct Attempts {
    count: usize,
    authenticated: bool,
    first_retired: bool,
    offline: bool,
    identity_preserved: bool,
    reason: String,
}

/// Retains actual wire observations after every original owner is closed.
async fn attempts(first_reason: ConnectionDisconnectFailReason) -> Attempts {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute {
            mut listener,
            target,
            http,
            ..
        } = fixture_route(Route::LocalHttp).await;
        let settings = crate::LoginSettings::default();
        let session = SessionLogin {
            execution: None,
            display_name: "Synthetic",
            client_random_id: crate::ClientRandomId::new(1).unwrap(),
            cache: None,
            skin: None,
            settings: &settings,
            tap: None,
            relay: None,
            target: None,
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let (finished, mut completion) = tokio::sync::oneshot::channel();
        let client = async {
            let result = super::negotiate(
                &target,
                &session,
                Some(provider::authentication()),
                deadline,
            )
            .await;
            let _ = finished.send(());
            result
        };
        let server = async {
            let mut first = listener.accept().await.unwrap();
            let (codec, request) = login(&mut first).await;
            let authenticated = bedrock_net::login::verify_authenticated_request(
                &request.connection_request,
                &provider::verifier(),
                UNIX_EPOCH + Duration::from_secs(provider::SERVICE_SECONDS),
                BatchLimits::default().max_decoded_bytes,
            )
            .ok();
            let original_uuid = authenticated
                .as_ref()
                .and_then(|identity| identity.player_identity())
                .map(|player| player.uuid().to_string());
            let authenticated = authenticated.is_some_and(|identity| {
                identity.xuid() == provider::XUID
                    && identity.public_key() == provider::key().verifying_key()
            });
            reject(&first, &codec, first_reason).await;
            let next = tokio::select! {
                peer = listener.accept() => Some(peer.unwrap()),
                _ = &mut completion => None,
            };
            let Some(mut second) = next else {
                drop(first.close());
                return (1, authenticated, false, false, false);
            };
            let (codec, request) = login(&mut second).await;
            let first_retired = first.session.closed().now_or_never().is_some();
            let offline = bedrock_net::login::parse_offline_request(
                &request.connection_request,
                SystemTime::now(),
                BatchLimits::default().max_decoded_bytes,
                |_| Ok(()),
            )
            .ok();
            let identity_preserved = offline.as_ref().is_some_and(|identity| {
                original_uuid.as_deref() == Some(identity.claimed_identity())
                    && identity.claimed_xuid().is_empty()
                    && identity.claimed_playfab_title_id().is_empty()
            });
            let offline = offline.is_some();
            reject(&second, &codec, ConnectionDisconnectFailReason::Serverfull).await;
            let count = tokio::select! {
                peer = listener.accept() => {
                    drop(peer.unwrap().close());
                    3
                },
                _ = &mut completion => 2,
            };
            drop(second.close());
            drop(first.close());
            (
                count,
                authenticated,
                first_retired,
                offline,
                identity_preserved,
            )
        };
        let (result, observed) = tokio::join!(client, server);
        listener.close().await.unwrap();
        drop(listener);
        Arc::try_unwrap(http.unwrap())
            .ok()
            .expect("HTTP fixture ownership leaked")
            .shutdown()
            .await;
        let error = result.err().expect("the fixture rejects every attempt");
        let reason = error
            .server_disconnect()
            .map(|event| event.reason)
            .unwrap_or_default();
        Attempts {
            count: observed.0,
            authenticated: observed.1,
            first_retired: observed.2,
            offline: observed.3,
            identity_preserved: observed.4,
            reason,
        }
    })
    .await
    .expect("finite actual local HTTP retry fixture stalled")
}

#[tokio::test]
async fn local_http_keyless_retry_retires_original_and_remains_offline_and_bounded() {
    let attempt = attempts(ConnectionDisconnectFailReason::Hostwithoutkeys).await;
    assert_eq!(
        attempt.count, 2,
        "a typed keyless local rejection must retry exactly once"
    );
    assert!(
        attempt.authenticated,
        "the first actual Login must retain provider authentication"
    );
    assert!(
        attempt.first_retired,
        "the first association must retire before the second Login"
    );
    assert!(attempt.offline, "the retry must use a fresh offline Login");
    assert!(
        attempt.identity_preserved,
        "offline retry must preserve the original player UUID without account claims"
    );
    assert_eq!(
        attempt.reason, "Serverfull",
        "the second rejection must be returned without another retry"
    );
}

#[tokio::test]
async fn local_http_disconnect_text_does_not_authorize_offline_retry() {
    let attempt = attempts(ConnectionDisconnectFailReason::Serverfull).await;
    assert_eq!(attempt.count, 1);
    assert!(attempt.authenticated);
    assert_eq!(attempt.reason, "Serverfull");
}
