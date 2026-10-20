use super::*;
use crate::login::native_network::{
    negotiated::Credentials,
    transport::{
        Transport,
        early_spawn_tests::{receive, send},
    },
};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, CompressionAlgorithm, CompressionSettings,
};
use bedrock_protocol_1_26_51::{packets, types::ActorRuntimeID, wire};
use bytes::Bytes;
use futures::FutureExt;
use nethernet::identity::{SdpIdentity, TokenTrust};
use nethernet_tokio::{ConnectionConfig, LanConfig, LanSignaling, NetherServer, ServerData};
use std::{net::SocketAddr, sync::Arc, time::SystemTime};

const NONCE: &str = "original-signed-lan-nonce";

/// Uses the same actual configured provider/clock as the caller's verified credential.
fn proof() -> crate::login::native_network::transport::identity::TransportIdentity {
    Credentials::new(Some(authentication()))
        .unwrap()
        .transport_identity()
        .unwrap()
}

/// Reuses real directed discovery and an accepting RTC owner, never a constructed Ready.
async fn listener(
    online: bool,
    self_signed: bool,
    identity: Option<Arc<dyn SdpIdentity>>,
) -> (NetherServer, SocketAddr) {
    let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let signaling = LanSignaling::with_config(
        31,
        address,
        LanConfig {
            discovery_port: address.port(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut data = ServerData::new("Fixture".into(), "World".into());
    data.flag_a = online;
    data.flag_b = self_signed;
    data.session_id = NONCE.into();
    data.connection_type = crate::login::native_network::transport::LAN_CONNECTION_TYPE;
    data.protocol_version = crate::PROTOCOL_VERSION as u32;
    signaling.set_server_data(data);
    let TokenTrust::ServerAnswer(provider) = proof().trust else {
        panic!("the qualified transport policy must retain its original provider");
    };
    let listener = NetherServer::bind_with(
        signaling,
        ConnectionConfig {
            identity,
            token_trust: Some(TokenTrust::Provider(provider)),
            allow_unsigned_answers: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    (listener, address)
}

/// Supplies an actual server fingerprint signer that the existing ServerAnswer policy verifies.
fn server_identity() -> Arc<dyn SdpIdentity> {
    Arc::new(nethernet::identity::ServerIdentity::generate("self", SystemTime::now()).unwrap())
}

/// Login verification still checks the actual provider signature before returning the signed key/XUID.
struct Provider;

impl bedrock_net::login::LoginVerifier for Provider {
    fn verify(
        &self,
        token: &str,
        now: SystemTime,
    ) -> Result<bedrock_net::login::TrustedIdentity, bedrock_net::login::LoginError> {
        use bedrock_net::login::{LoginError, TrustedIdentity};
        let claims = nethernet::identity::TokenVerifier::verify(&verifier(), token, now)
            .map_err(|_| LoginError::Authentication)?;
        let xuid = claims.values()["xid"]
            .as_str()
            .ok_or(LoginError::Authentication)?;
        let key = claims
            .client_public_key()
            .map_err(|_| LoginError::Authentication)?;
        TrustedIdentity::new(xuid.into(), key)
    }
}

#[tokio::test]
async fn signed_lan_public_join_keeps_provider_proof_nonce_startup_and_bidirectional_play() {
    tokio::time::timeout(
        Duration::from_secs(20),
        Box::pin(async {
            let (mut listener, address) = listener(true, false, Some(server_identity())).await;
            let server = async {
                // The listener requires the actual provider token/fingerprint before yielding this owner.
                let mut peer = listener.accept().await.unwrap();
                let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
                let request = receive(&mut peer, &codec).await;
                assert!(matches!(
                    &*request[0].data,
                    packets::Packet::RequestNetworkSettings(_)
                ));
                send(
                    &peer,
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
                let received = receive(&mut peer, &codec).await;
                let packets::Packet::Login(login) = &*received[0].data else {
                    panic!("the original native handshake must send Login");
                };
                let login = bedrock_net::login::verify_authenticated_request(
                    &login.connection_request,
                    &Provider,
                    UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS),
                    BatchLimits::default().max_decoded_bytes,
                )
                .unwrap();
                assert_eq!(login.xuid(), XUID);
                assert_eq!(login.public_key(), key().verifying_key());
                assert_eq!(login.nonce(), NONCE);
                let claims: Value = serde_json::from_str(login.client_data().get()).unwrap();
                assert_eq!(claims["ServerAddress"], address.to_string());
                assert_eq!(claims["ThirdPartyName"], "Synthetic");
                send(
                    &peer,
                    &codec,
                    &[
                        crate::Packet::new(packets::PlayStatus {
                            status: PlayStatusType::Loginsuccess,
                        }),
                        crate::Packet::new(packets::ResourcePacksInfo::default()),
                    ],
                )
                .await;
                assert!(matches!(
                    &*receive(&mut peer, &codec).await[0].data,
                    packets::Packet::ClientCacheStatus(_)
                ));
                assert!(matches!(
                    &*receive(&mut peer, &codec).await[0].data,
                    packets::Packet::ResourcePackClientResponse(_)
                ));
                send(
                    &peer,
                    &codec,
                    &[
                        crate::Packet::new(packets::ResourcePackStack::default()),
                        crate::Packet::new(packets::StartGame {
                            runtime_id: ActorRuntimeID(42),
                            ..Default::default()
                        }),
                        crate::Packet::new(packets::ItemRegistry::default()),
                        crate::Packet::new(packets::ChunkRadiusUpdated {
                            chunk_radius: wire::ZigZag32(8),
                        }),
                        crate::Packet::new(packets::PlayStatus {
                            status: PlayStatusType::Playerspawn,
                        }),
                    ],
                )
                .await;
                assert!(matches!(
                    &*receive(&mut peer, &codec).await[0].data,
                    packets::Packet::ResourcePackClientResponse(_)
                ));
                assert!(matches!(
                    &*receive(&mut peer, &codec).await[0].data,
                    packets::Packet::RequestChunkRadius(_)
                ));
                let initialized = receive(&mut peer, &codec).await;
                assert_eq!(
                    initialized
                        .iter()
                        .filter(|packet| matches!(&*packet.data,
                            packets::Packet::SetLocalPlayerAsInitialized(value) if value.player_id.0 == 42))
                        .count(),
                    1,
                    "loading completion must initialize the original player exactly once"
                );
                assert!(matches!(&*receive(&mut peer, &codec).await[0].data,
                packets::Packet::RequestChunkRadius(value) if value.chunk_radius.0 == 7));
                send(
                    &peer,
                    &codec,
                    &[crate::Packet::new(packets::SetTime {
                        time: wire::ZigZag32(23),
                    })],
                )
                .await;
                peer.session.closed().await;
            };
            let client = async {
                let settings = crate::LoginSettings::default();
                let (mut play, runtime_id) = LoginSequence::connect_native_authenticated(
                    &NativeConnectionTarget::new(
                        address.to_string(),
                        NativeTransport::NetherNetLan,
                    ),
                    SessionLogin {
                        display_name: "untrusted application name",
                        client_random_id: crate::ClientRandomId::new(1).unwrap(),
                        cache: None,
                        skin: None,
                        settings: &settings,
                        tap: None,
                        #[cfg(not(target_arch = "wasm32"))]
                        relay: None,
                        target: None,
                    },
                    authentication(),
                    |_, game| game.world.local_player_runtime_id,
                )
                .await
                .unwrap();
                assert_eq!(runtime_id, 42);
                assert!(matches!(
                    &*play.recv().await.unwrap().data,
                    packets::Packet::ChunkRadiusUpdated(_)
                ));
                play.finish_loading().await.unwrap();
                play.send(crate::Packet::new(packets::RequestChunkRadius {
                    chunk_radius: wire::ZigZag32(7),
                    ..Default::default()
                }))
                .await
                .unwrap();
                assert!(matches!(&*play.recv().await.unwrap().data,
                packets::Packet::SetTime(value) if value.time.0 == 23));
                play.close().await.unwrap();
            };
            tokio::join!(server, client);
            listener.close().await.unwrap();
        }),
    )
    .await
    .expect("finite actual signed LAN startup stalled");
}

struct MalformedAnswer;

impl SdpIdentity for MalformedAnswer {
    fn augment(&self, sdp: &str, _: SystemTime) -> nethernet::identity::error::Result<String> {
        Ok(format!("{sdp}a=identity:malformed-present-assertion\r\n"))
    }
}

#[tokio::test]
async fn signed_lan_refuses_offline_only_and_missing_or_malformed_answers_without_fallback() {
    tokio::time::timeout(Duration::from_secs(20), async {
        for (online, identity) in [
            (false, Some(server_identity())),
            (true, None),
            (
                true,
                Some(Arc::new(MalformedAnswer) as Arc<dyn SdpIdentity>),
            ),
        ] {
            let (mut listener, address) = listener(online, true, identity).await;
            let proof = proof();
            let result = Transport::connect(
                &NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan),
                None,
                Some(&proof),
                tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
            )
            .await;
            let error = result
                .err()
                .expect("signed LAN downgraded its admission policy");
            let accepted = listener.accept().now_or_never().is_some();
            listener.close().await.unwrap();
            assert!(!accepted, "rejected signed LAN left an accepted peer");
            if online {
                assert!(
                    matches!(
                        error
                            .get_ref()
                            .and_then(|error| error.downcast_ref::<nethernet_tokio::NetherError>()),
                        Some(nethernet_tokio::NetherError::Identity(_))
                    ),
                    "present or absent signed answer escaped its trust boundary: {error}"
                );
            } else {
                assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            }
        }
    })
    .await
    .expect("finite rejected signed LAN fixture stalled");
}

#[tokio::test]
async fn signed_lan_expiry_rejects_association_and_send_without_losing_the_ready_owner() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let (mut listener, address) = listener(true, false, Some(server_identity())).await;
        let target =
            NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan);
        let mut expired = proof();
        expired.expires = Some(Instant::now());
        let result = Transport::connect(
            &target,
            None,
            Some(&expired),
            tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
        )
        .await;
        assert!(authentication::is_expired(
            &result
                .err()
                .expect("expired proof admitted a LAN association")
        ));
        let proof = proof();
        let (connected, accepted) = tokio::join!(
            Transport::connect(
                &target,
                None,
                Some(&proof),
                tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT
            ),
            listener.accept(),
        );
        let mut connected = connected.unwrap();
        let mut peer = accepted.unwrap();
        let expired = connected
            .send_until(Bytes::from_static(b"expired Login"), Some(Instant::now()))
            .await;
        connected
            .send_until(Bytes::from_static(b"original valid prefix"), proof.expires)
            .await
            .unwrap();
        let observed = peer.reliable.recv().await.unwrap().unwrap();
        connected.close().await.unwrap();
        peer.close().session.closed().await;
        listener.close().await.unwrap();
        assert!(authentication::is_expired(&expired.unwrap_err()));
        assert_eq!(observed, b"original valid prefix"[..]);
    })
    .await
    .expect("finite actual signed LAN expiry control stalled");
}
