use super::super::{LoginSequence, SessionLogin};
use super::*;
use bedrock_net::{
    BatchCodec, BatchLimits, CompressionAlgorithm, CompressionSettings, DecodeOutcome,
};
use bedrock_protocol_1_26_51::{enums::PlayStatusType, packets, types::ActorRuntimeID, wire};
use nethernet_tokio::{AcceptedSession, NetherServer, ServerData};
use std::sync::Arc;
use std::time::Duration;

/// Sends generated packets through the same bounded batch codec used by native connections.
pub(in crate::login::native_network) async fn send<S: bedrock_net::BatchPhase>(
    peer: &AcceptedSession,
    codec: &BatchCodec<S>,
    packets: &[crate::Packet],
) {
    let body = crate::session_transport::batch_body(packets).unwrap();
    let entries = wire::Frames::new(body, BatchLimits::default().max_packets)
        .map(|entry| {
            let entry = entry.unwrap();
            let mut reader = wire::Reader::from_shared(entry.frame());
            reader.read_var_u32().unwrap();
            entry.frame().slice(reader.position()..)
        })
        .collect::<Vec<_>>();
    for message in codec.encode(&entries).unwrap() {
        peer.session.send(message).await.unwrap();
    }
}

/// Reads one real reliable message and decodes its original generated packet entries.
pub(in crate::login::native_network) async fn receive<S: bedrock_net::BatchPhase>(
    peer: &mut AcceptedSession,
    codec: &BatchCodec<S>,
) -> Vec<crate::Packet> {
    let message = peer.reliable.recv().await.unwrap().unwrap();
    let DecodeOutcome::Packets { packets, .. } = codec.decode(message).unwrap() else {
        panic!("the native client must send a valid batch");
    };
    let mut batch = vec![wire::BATCH_HEADER];
    for packet in packets {
        wire::Writer::with_limit(&mut batch, BatchLimits::default().max_decoded_bytes)
            .write_len_var_u32(packet.len())
            .unwrap();
        batch.extend_from_slice(&packet);
    }
    crate::codec::decode_batch(Bytes::from(batch)).unwrap()
}

#[tokio::test]
async fn native_pack_deferred_spawn_reaches_startup_once_with_siblings() {
    pack_deferred_spawn(Route::Lan).await;
}

#[tokio::test]
async fn selected_local_lan_preserves_offline_login_nonce_and_actual_network_id_through_startup() {
    pack_deferred_spawn(Route::LocalLan).await;
}

/// HTTP discovery and an actual pending trust decision precede the original Login/packs/spawn path.
#[tokio::test]
async fn addressed_http_trust_precedes_login_and_preserves_original_startup_and_live_identity() {
    pack_deferred_spawn(Route::AddressedHttp).await;
}

#[derive(Clone, Copy)]
pub(in crate::login::native_network) enum Route {
    Lan,
    LocalLan,
    AddressedHttp,
    LocalHttp,
}

/// Exercises the same original Login, packs and live owner for each actual selected transport.
async fn pack_deferred_spawn(route: Route) {
    tokio::time::timeout(Duration::from_secs(20), async {
        let FixtureRoute { mut listener, address, target, http, approval } = fixture_route(route).await;
        let first = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(1),
        });
        let second = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(2),
        });
        let third = crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(3),
        });
        let radius = crate::Packet::new(packets::ChunkRadiusUpdated {
            chunk_radius: wire::ZigZag32(8),
        });
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
            let settings = if let Some((shown, approved)) = approval {
                shown.await.unwrap();
                let mut settings = Box::pin(receive(&mut peer, &codec));
                assert!(futures::poll!(settings.as_mut()).is_pending(), "protocol output preceded the actual trust decision");
                approved.send(true).unwrap();
                settings.await
            } else { receive(&mut peer, &codec).await };
            assert!(matches!(&*settings[0].data, packets::Packet::RequestNetworkSettings(_)));
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
            let login_packets = receive(&mut peer, &codec).await;
            let packets::Packet::Login(login) = &*login_packets[0].data else {
                panic!("the real client must send Login");
            };
            let expected_address = match route {
                Route::LocalLan => "31".into(),
                Route::AddressedHttp | Route::LocalHttp => format!("http://{address}:{}", address.port()),
                Route::Lan => address.to_string(),
            };
            let nonce = if matches!(route, Route::AddressedHttp | Route::LocalHttp) { "" } else { "synthetic-nonce" };
            let identity = offline_login_identity(login, &expected_address, nonce);
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
                    crate::Packet::new(packets::PlayStatus {
                        status: PlayStatusType::Playerspawn,
                    }),
                    first.clone(),
                    second.clone(),
                    crate::Packet::new(packets::ResourcePackStack::default()),
                    crate::Packet::new(packets::StartGame {
                        runtime_id: ActorRuntimeID(42),
                        ..Default::default()
                    }),
                    crate::Packet::new(packets::ItemRegistry::default()),
                    radius.clone(),
                    third.clone(),
                ],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));
            let requested = receive(&mut peer, &codec).await;
            assert!(matches!(
                &*requested[0].data,
                packets::Packet::RequestChunkRadius(_)
            ));
            let initialized = receive(&mut peer, &codec).await;
            assert_eq!(
                initialized
                    .iter()
                    .filter(|packet| matches!(&*packet.data, packets::Packet::SetLocalPlayerAsInitialized(value) if value.player_id.0 == 42))
                    .count(),
                1
            );
            let outgoing = receive(&mut peer, &codec).await;
            let mut expected = identity_packets();
            let packets::Packet::PlayerSkin(skin) = &mut *expected[0].data else {
                unreachable!();
            };
            skin.uuid = identity;
            assert_eq!(outgoing.len(), 1);
            let packets::Packet::PlayerSkin(actual) = &*outgoing[0].data else {
                panic!("expected the original skin update");
            };
            assert_eq!(actual.uuid, identity, "skin UUID must match actual Login");
            assert!(outgoing == expected[..1], "skin metadata, pixels and headers must survive");
            let outgoing = receive(&mut peer, &codec).await;
            let packets::Packet::Text(chat) = &mut *expected[1].data else {
                unreachable!();
            };
            let bedrock_protocol_1_26_51::types::TextData::AuthorAndMessage { player_name, .. } = &mut chat.body else {
                unreachable!();
            };
            *player_name = "Fixture".into();
            chat.sender_xuid = Default::default();
            assert_eq!(outgoing, expected[1..], "chat identity changes without changing siblings or headers");
            peer.session.closed().await;
        };
        let client = async {
            let settings = super::super::super::LoginSettings::default();
            let (mut play, runtime_id) = LoginSequence::connect_native(
                &target,
                SessionLogin {
                    display_name: "Fixture",
                    client_random_id: crate::ClientRandomId::new(1).unwrap(),
                    cache: None,
                    skin: None,
                    settings: &settings,
                    tap: None,
                    #[cfg(not(target_arch = "wasm32"))]
                    relay: None,
                    target: None,
                },
                |_, game| game.world.local_player_runtime_id,
            )
            .await
            .expect("early startup input must survive the actual native pack consumer");
            assert_eq!(runtime_id, 42);
            for expected in [first.clone(), second.clone(), radius.clone(), third.clone()] {
                assert_eq!(play.recv().await.unwrap(), expected);
            }
            play.finish_loading().await.unwrap();
            play.finish_loading().await.unwrap();
            let mut outgoing = identity_packets();
            play.send(outgoing.remove(0)).await.unwrap();
            play.outbound().unwrap().send_batch(outgoing, None).await.unwrap();
            play.close().await.unwrap();
        };
        tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        if let Some(http) = http {
            Arc::try_unwrap(http).ok().expect("original HTTP listener leaked").shutdown().await;
        }
    })
    .await
    .expect("finite actual native pack/startup fixture stalled");
}

pub(in crate::login::native_network) struct FixtureRoute {
    pub(in crate::login::native_network) listener: NetherServer,
    pub(in crate::login::native_network) address: SocketAddr,
    pub(in crate::login::native_network) target: NativeConnectionTarget,
    pub(in crate::login::native_network) http:
        Option<std::sync::Arc<nethernet_tokio::signaling::http::HttpSignalingServer>>,
    pub(in crate::login::native_network) approval: Option<(
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<bool>,
    )>,
}

struct HttpApproval {
    key: p384::ecdsa::VerifyingKey,
    endpoint: String,
    shown: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    approved: std::sync::Mutex<Option<tokio::sync::oneshot::Receiver<bool>>>,
}

impl crate::NativeServerTrust for HttpApproval {
    fn trust<'a>(
        &'a self,
        endpoint: &'a str,
        key: &'a p384::ecdsa::VerifyingKey,
        _: Instant,
    ) -> futures::future::BoxFuture<'a, io::Result<bool>> {
        Box::pin(async move {
            assert_eq!(endpoint, self.endpoint);
            assert_eq!(
                *key, self.key,
                "trust must receive the actually verified server proof key"
            );
            self.shown
                .lock()
                .unwrap()
                .take()
                .expect("original trust decision repeated")
                .send(())
                .unwrap();
            let approved = self.approved.lock().unwrap().take().unwrap();
            Ok(approved.await.unwrap())
        })
    }
}

/// Reuses the original finite server owner and actual wire script; only route acquisition differs.
pub(in crate::login::native_network) async fn fixture_route(route: Route) -> FixtureRoute {
    if matches!(route, Route::AddressedHttp | Route::LocalHttp) {
        use nethernet_tokio::signaling::http::{HttpServerConfig, HttpSignalingServer};
        let identity = std::sync::Arc::new(
            nethernet::identity::ServerIdentity::generate("self", std::time::SystemTime::now())
                .unwrap(),
        );
        let signaling = std::sync::Arc::new(
            HttpSignalingServer::bind("127.0.0.1:0".parse().unwrap(), HttpServerConfig::default())
                .await
                .unwrap(),
        );
        signaling.set_server_data(ServerData::new("Fixture".into(), "World".into()));
        let address = signaling.local_addr();
        let listener = NetherServer::bind_with(
            std::sync::Arc::clone(&signaling),
            ConnectionConfig {
                identity: Some(identity.clone()),
                server_ice: Some(nethernet_tokio::ServerIceConfig {
                    turn_streams: Vec::new(),
                    turn_tls: None,
                    ice: Default::default(),
                    bind_addr: "127.0.0.1:0".parse().unwrap(),
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        if matches!(route, Route::LocalHttp) {
            return FixtureRoute {
                listener,
                address,
                target: NativeConnectionTarget::new(
                    address.to_string(),
                    NativeTransport::LocalNetherNetHttp,
                ),
                http: Some(signaling),
                approval: None,
            };
        }
        let (shown, question) = tokio::sync::oneshot::channel();
        let (answer, approved) = tokio::sync::oneshot::channel();
        let trust = std::sync::Arc::new(HttpApproval {
            key: *identity.verifying_key(),
            endpoint: format!("http://{address}"),
            shown: std::sync::Mutex::new(Some(shown)),
            approved: std::sync::Mutex::new(Some(approved)),
        });
        let target = NativeConnectionTarget::addressed(address.to_string(), trust).unwrap();
        return FixtureRoute {
            listener,
            address,
            target,
            http: Some(signaling),
            approval: Some((question, answer)),
        };
    }
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
    data.flag_b = true;
    data.session_id = "synthetic-nonce".into();
    data.connection_type = LAN_CONNECTION_TYPE;
    data.protocol_version = if matches!(route, Route::LocalLan) {
        0
    } else {
        crate::PROTOCOL_VERSION as u32
    };
    signaling.set_server_data(data);
    let listener = NetherServer::bind_with(
        signaling,
        ConnectionConfig {
            allow_unsigned_answers: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let target = if matches!(route, Route::LocalLan) {
        NativeConnectionTarget::local_lan(address, "World".into(), crate::NATIVE_JOIN_TIMEOUT)
            .unwrap()
    } else {
        NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan)
    };
    FixtureRoute {
        listener,
        address,
        target,
        http: None,
        approval: None,
    }
}

/// Observes the UUID in this real offline Login without treating the fixture token as authority.
pub(in crate::login::native_network) fn offline_login_identity(
    login: &packets::Login,
    address: &str,
    nonce: &str,
) -> uuid::Uuid {
    use base64::Engine;
    let bytes = &login.connection_request;
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let envelope: serde_json::Value = serde_json::from_slice(&bytes[4..4 + length]).unwrap();
    assert_eq!(envelope["AuthenticationType"], 2);
    let token = envelope["Token"].as_str().unwrap();
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.split('.').nth(1).unwrap())
        .unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(claims["xid"], "");
    let client_offset = 4 + length;
    let client_length =
        u32::from_le_bytes(bytes[client_offset..client_offset + 4].try_into().unwrap()) as usize;
    let client =
        std::str::from_utf8(&bytes[client_offset + 4..client_offset + 4 + client_length]).unwrap();
    let client = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(client.split('.').nth(1).unwrap())
        .unwrap();
    let client: serde_json::Value = serde_json::from_slice(&client).unwrap();
    assert_eq!(client["Nonce"], nonce);
    assert_eq!(client["ServerAddress"], address);
    uuid::Uuid::parse_str(claims["leguuid"].as_str().unwrap()).unwrap()
}

/// Exercises the synthetic local skin identity, chat correction, and unchanged nonchat/FIFO headers.
fn identity_packets() -> Vec<crate::Packet> {
    use bedrock_protocol_1_26_51::enums::TextPacketType;
    use bedrock_protocol_1_26_51::types::{SerializedSkinRef, SkinImage, TextData};
    let mut skin = crate::Packet::new(packets::PlayerSkin {
        uuid: uuid::Uuid::nil(),
        serialized_skin: SerializedSkinRef {
            image_data: SkinImage {
                width: wire::U32LE(render_api::CLASSIC_SKIN_SIDE as u32),
                height: wire::U32LE(render_api::CLASSIC_SKIN_SIDE as u32),
                image_bytes: vec![
                    255;
                    render_api::CLASSIC_SKIN_SIDE * render_api::CLASSIC_SKIN_SIDE * 4
                ]
                .into(),
            },
            ..Default::default()
        },
        ..Default::default()
    });
    skin.header.from_subclient = 1;
    skin.header.to_subclient = 2;
    let chat = crate::chat_text_packet("Synthetic", "123", "same message").unwrap();
    let system = crate::Packet::new(packets::Text {
        body: TextData::MessageOnly {
            message_type: TextPacketType::Systemmessage,
            message: "unchanged".into(),
        },
        sender_xuid: "456".into(),
        ..Default::default()
    });
    vec![
        skin,
        chat,
        system,
        crate::Packet::new(packets::SetTime {
            time: wire::ZigZag32(9),
        }),
    ]
}
