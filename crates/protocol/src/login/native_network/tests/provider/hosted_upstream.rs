//! Original local HTTP/LAN upstream and real signed guest startup; no world or Ready is fabricated.

use super::*;
mod cdn;
use crate::login::native_network::{
    NativeAcceptingSend, NativeHostedRelayProgress, NativeHostedUpstream,
    accepting::send::tests::{Pair, signed_pair_with_key},
    transport::early_spawn_tests::{
        FixtureRoute, Route, fixture_route, offline_login_identity, receive, send,
    },
};
use bedrock_client::{
    handshake::{
        ClientHandshake, ClientLogin, HandshakeOptions, LoginEvent, PacksEvent, SettingsEvent,
    },
    server::{
        ServerApprovalEvent, ServerLoginEvent, ServerPacksReady, ServerSettingsEvent,
        session::{ServerPackDelivery, ServerPackEvent, ServerWorldReady},
    },
};
use bedrock_net::{
    BatchCodec, BatchHeader, BatchLimits, CompressionAlgorithm, CompressionSettings, DecodeOutcome,
};
use bedrock_protocol_1_26_51::{
    enums::PlayStatusType,
    packets,
    types::{ActorRuntimeID, PackInfoData, PackInstanceId},
    wire,
};
use bytes::Bytes;
use resource_pack::test_support::archive_bytes;
use sha2::{Digest, Sha256};
use std::sync::Arc;

const TIMEOUT: Duration = Duration::from_secs(20);
const PACK_ID: uuid::Uuid = uuid::Uuid::from_u128(77);
const VERSION: &str = "1.0.0";
const GUEST_ID: uuid::Uuid = uuid::Uuid::from_u128(1234);

#[tokio::test]
async fn local_bds_http_and_selected_lan_keep_guest_packs_raw_startup_and_both_original_wires() {
    for route in [Route::LocalHttp, Route::LocalLan] {
        tokio::time::timeout(TIMEOUT, original_upstream(route))
            .await
            .expect("finite original local BDS relay fixture stalled");
    }
}

/// Uses the real provider Login, pack source, two actual associations and initialized flush.
async fn original_upstream(route: Route) {
    original_upstream_with_cdn(route, None).await;
}

/// Keeps both original legs and startup while optionally acquiring the upstream archive by CDN.
async fn original_upstream_with_cdn(route: Route, cdn: Option<std::net::SocketAddr>) {
    original_upstream_with_archive(
        route,
        cdn,
        archive_bytes(PACK_ID, &[]),
        bridge::contract::RESOURCE_PACK_TYPE_RESOURCES,
    )
    .await;
}

/// Carries one original fixture archive through the same upstream and signed guest owners.
async fn original_upstream_with_archive(
    route: Route,
    cdn: Option<std::net::SocketAddr>,
    archive: Vec<u8>,
    pack_type: u8,
) {
    let FixtureRoute {
        mut listener,
        address,
        target,
        http,
        approval,
    } = fixture_route(route).await;
    assert!(approval.is_none());
    let mut offer = PackInfoData::default();
    offer.pack_id_version.pack_uuid = PACK_ID;
    offer.pack_id_version.pack_version.version = VERSION.into();
    offer.pack_size = wire::U64LE(archive.len() as u64);
    offer.subpack_name = "original-offer-subpack".into();
    if let Some(endpoint) = cdn {
        offer.cdn_url = format!("http://{endpoint}/only-archive").into();
    }
    let info = packets::ResourcePacksInfo {
        resource_pack_required: true,
        has_scripts: true,
        has_addon_packs: true,
        force_disable_vibrant_visuals: true,
        world_template_id_and_version: bedrock_protocol_1_26_51::types::PackIdVersion {
            pack_uuid: uuid::Uuid::from_u128(99),
            ..Default::default()
        },
        resource_packs: vec![offer],
    };
    let stack = packets::ResourcePackStack {
        texture_pack_required: true,
        texture_pack_list: vec![PackInstanceId {
            pack_id: PACK_ID.to_string().into(),
            version: VERSION.into(),
            sub_pack_name: "original-selected-subpack".into(),
        }],
        base_game_version: crate::GAME_VERSION.into(),
        ..Default::default()
    };
    let bds = async {
        let mut peer = listener.accept().await.unwrap();
        let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
        assert!(matches!(
            &*receive(&mut peer, &codec).await[0].data,
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
        let login = receive(&mut peer, &codec).await;
        let packets::Packet::Login(login) = &*login[0].data else {
            panic!("actual offline Login missing")
        };
        let label = match route {
            Route::LocalLan => "31".into(),
            Route::LocalHttp => format!("http://{address}:{}", address.port()),
            _ => unreachable!(),
        };
        assert_eq!(
            offline_login_identity(
                login,
                &label,
                if matches!(route, Route::LocalLan) {
                    "synthetic-nonce"
                } else {
                    ""
                }
            ),
            GUEST_ID
        );
        assert_offline_name(login);
        send(
            &peer,
            &codec,
            &[
                crate::Packet::new(packets::PlayStatus {
                    status: PlayStatusType::Loginsuccess,
                }),
                crate::Packet::new(info.clone()),
            ],
        )
        .await;
        assert!(matches!(
            &*receive(&mut peer, &codec).await[0].data,
            packets::Packet::ClientCacheStatus(_)
        ));
        let response = receive(&mut peer, &codec).await;
        assert!(matches!(
            &*response[0].data,
            packets::Packet::ResourcePackClientResponse(_)
        ));
        if cdn.is_some() {
            // The upstream never emits DataInfo or chunks for this actual preloaded archive.
            assert!(matches!(
                &*response[0].data,
                packets::Packet::ResourcePackClientResponse(packets::ResourcePackClientResponse {
                    response: bedrock_protocol_1_26_51::types::ResourcePackClientResponseData::DownloadingFinished { .. },
                })
            ));
        } else {
            let data = packets::ResourcePackDataInfo {
                resource_name: format!("{PACK_ID}_{VERSION}").into(),
                chunk_size: wire::U32LE(archive.len() as u32),
                number_of_chunks: wire::U32LE(1),
                file_size: wire::U64LE(archive.len() as u64),
                file_hash: Bytes::copy_from_slice(&Sha256::digest(&archive)),
                is_premium_pack: true,
                pack_type: wire::U8(pack_type),
            };
            send(&peer, &codec, &[crate::Packet::new(data)]).await;
            let chunk = receive(&mut peer, &codec).await;
            let packets::Packet::ResourcePackChunkRequest(chunk) = &*chunk[0].data else {
                panic!("actual original archive request missing")
            };
            assert_eq!(chunk.chunk.0, 0);
            send(
                &peer,
                &codec,
                &[crate::Packet::new(packets::ResourcePackChunkData {
                    resource_name: chunk.resource_name.clone(),
                    chunk_id: wire::U32LE(0),
                    byte_offset: wire::U64LE(0),
                    chunk_data: Bytes::copy_from_slice(&archive),
                })],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));
        }
        let mut secondary = crate::Packet::new(packets::StartGame {
            runtime_id: ActorRuntimeID(999),
            ..Default::default()
        });
        secondary.header.from_subclient = 1;
        send(
            &peer,
            &codec,
            &[
                crate::Packet::new(packets::PlayStatus {
                    status: PlayStatusType::Playerspawn,
                }),
                crate::Packet::new(stack.clone()),
                secondary,
                crate::Packet::new(packets::SetTime {
                    time: wire::ZigZag32(7),
                }),
                crate::Packet::new(packets::StartGame {
                    runtime_id: ActorRuntimeID(42),
                    ..Default::default()
                }),
                crate::Packet::new(packets::ItemRegistry::default()),
                crate::Packet::new(packets::ChunkRadiusUpdated {
                    chunk_radius: wire::ZigZag32(8),
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
        assert!(
            matches!(&*initialized[0].data, packets::Packet::SetLocalPlayerAsInitialized(value) if value.player_id.0 == 42)
        );
        let live = receive(&mut peer, &codec).await;
        assert!(matches!(&*live[0].data, packets::Packet::SetTime(value) if value.time.0 == 11));
        send(
            &peer,
            &codec,
            &[crate::Packet::new(packets::SetTime {
                time: wire::ZigZag32(12),
            })],
        )
        .await;
        peer.session.closed().await;
    };
    let relay = async {
        let (mut pair, handshake) = signed_pair_with_key(Arc::new(key())).await;
        let deadline = Instant::now() + TIMEOUT;
        let utc = UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS);
        let provider = verifier();
        let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
        let (client, request) = ClientHandshake::new(
            Arc::new(key()),
            HandshakeOptions {
                batch_encryption: false,
                ..Default::default()
            },
        )
        .unwrap()
        .start()
        .unwrap();
        client_send(&pair.client, &codec, vec![request.into()]).await;
        let ServerSettingsEvent::Settings(settings) = handshake
            .receive(
                pair.wire.receive(deadline).await.unwrap(),
                &provider,
                Instant::now(),
                utc,
            )
            .unwrap()
        else {
            panic!("actual settings continuation missing")
        };
        finish_send(
            pair.wire
                .prepare_packets(vec![settings.packet().clone().into()], deadline)
                .unwrap(),
        )
        .await;
        pair.wire
            .compression_flushed(settings.compression())
            .unwrap();
        let server = settings.flushed(Instant::now()).unwrap();
        let settings = client_receive(&mut pair.client, &codec).await.remove(0);
        let SettingsEvent::Ready {
            login: client,
            compression,
        } = client.receive(settings).unwrap()
        else {
            panic!("actual client settings missing")
        };
        let codec = codec.negotiate(compression);
        let mut claims = claims();
        claims["leguuid"] = json!(GUEST_ID.to_string());
        claims["mid"] = json!("provider-playfab");
        claims["tid"] = json!("provider-title");
        let token = signed(claims, "synthetic");
        let raw = RawValue::from_string(
            r#"{"ThirdPartyName":"untrusted name","Nonce":"original-guest-nonce","opaque":1e1}"#
                .into(),
        )
        .unwrap();
        let (client, login) = client.login_authenticated(&token, &raw).unwrap();
        client_send(&pair.client, &codec, vec![login.into()]).await;
        let ServerLoginEvent::Verified(login) = server
            .receive(
                pair.wire.receive(deadline).await.unwrap(),
                &provider,
                Instant::now(),
                utc,
            )
            .unwrap()
        else {
            panic!("actual provider verification missing")
        };
        assert_eq!(login.login().client_data().get(), raw.get());
        let ServerApprovalEvent::Ready(ready) = login
            .authorize(|_| Ok::<_, std::io::Error>(7_u8))
            .unwrap()
            .accept(&provider, None, Instant::now(), utc)
            .unwrap()
        else {
            panic!("DTLS-only association unexpectedly requested batch encryption")
        };
        assert!(
            matches!(
                NativeHostedUpstream::connect(&target, ready.login(), true, Instant::now()).await,
                Err(ProtocolError::Session(crate::SessionError(SessionFailure::NativeTransport(error))))
                    if error.kind() == std::io::ErrorKind::TimedOut
            ),
            "expired original cutoff opened another upstream"
        );
        let mut upstream = NativeHostedUpstream::connect(&target, ready.login(), true, deadline)
            .await
            .unwrap();
        assert!(
            upstream.release_received().is_err(),
            "uninitialized upstream released a play cursor"
        );
        assert!(
            upstream
                .prepare_raw(
                    raw_packet(
                        packets::SetTime {
                            time: wire::ZigZag32(999)
                        }
                        .into()
                    ),
                    deadline
                )
                .is_err(),
            "uninitialized upstream exposed application admission"
        );
        assert_eq!(
            upstream.read_pack(0, 0, archive.len()).unwrap().as_ref(),
            archive.as_slice()
        );
        assert!(upstream.read_pack(0, archive.len() as u64, 1).is_err());
        let snapshot = upstream.take_packs().unwrap();
        assert_eq!(snapshot.offer, info);
        assert_eq!(snapshot.stack, stack);
        assert_eq!(snapshot.contents.len(), 1);
        assert!(
            upstream.take_packs().is_err(),
            "original snapshot transferred twice"
        );
        let ready = guest_packs(
            ready,
            client,
            &mut pair,
            &codec,
            (&upstream, snapshot),
            &provider,
            (utc, deadline, cdn.is_none(), pack_type),
        )
        .await;
        let mut relay = upstream
            .relay_world(pair.wire, ready, &provider, Instant::now(), utc)
            .unwrap();
        for expected in [
            packets::PlayStatus::ID,
            packets::SetTime::ID,
            packets::StartGame::ID,
            packets::ItemRegistry::ID,
            packets::ChunkRadiusUpdated::ID,
        ] {
            relay.receive().await.unwrap();
            assert!(
                relay.complete_forward().is_err(),
                "unprepared forwarding advanced startup"
            );
            {
                let (login, grant, mut sending) = relay.prepare_forward().unwrap();
                while let Some(capacity) = sending.next().await.unwrap() {
                    login.revalidate(&provider, utc).unwrap();
                    assert_eq!(*grant, 7);
                    capacity.commit().unwrap();
                }
            }
            assert!(matches!(
                relay.complete_forward().unwrap(),
                NativeHostedRelayProgress::Forwarded
            ));
            let packet = client_receive(&mut pair.client, &codec).await.remove(0);
            assert_eq!(
                packet.id() as u32,
                expected,
                "upstream original FIFO/subclient filtering changed"
            );
        }
        for packet in [
            packets::Packet::from(packets::RequestChunkRadius {
                chunk_radius: wire::ZigZag32(8),
                max_chunk_radius: wire::U8(8),
            }),
            packets::SetLocalPlayerAsInitialized {
                player_id: ActorRuntimeID(42),
            }
            .into(),
        ] {
            let initializes = matches!(packet, packets::Packet::SetLocalPlayerAsInitialized(_));
            client_send(&pair.client, &codec, vec![packet]).await;
            relay.receive().await.unwrap();
            {
                let (login, grant, mut sending) = relay.prepare_forward().unwrap();
                let capacity = sending.next().await.unwrap().unwrap();
                login.revalidate(&provider, utc).unwrap();
                assert_eq!(*grant, 7);
                capacity.commit().unwrap();
            }
            assert!(
                relay.complete_forward().is_err(),
                "local enqueue was treated as original actor receipt completion"
            );
            {
                let (login, grant, mut sending) = relay.resume_forward().unwrap();
                while let Some(capacity) = sending.next().await.unwrap() {
                    login.revalidate(&provider, utc).unwrap();
                    assert_eq!(*grant, 7);
                    capacity.commit().unwrap();
                }
            }
            assert!(matches!(
                (initializes, relay.complete_forward().unwrap()),
                (false, NativeHostedRelayProgress::Forwarded)
                    | (true, NativeHostedRelayProgress::Initialized)
            ));
        }
        let (wire, mut upstream, accepted) = relay.finish(&provider, Instant::now(), utc).unwrap();
        assert_eq!(*accepted.grant(), 7);
        assert_eq!(accepted.login().player_identity().unwrap().uuid(), GUEST_ID);
        pair.wire = wire;
        client_send(
            &pair.client,
            &codec,
            vec![
                packets::SetTime {
                    time: wire::ZigZag32(11),
                }
                .into(),
            ],
        )
        .await;
        let raw = pair.wire.receive_play().await.unwrap();
        finish_send(upstream.prepare_raw(raw, deadline).unwrap()).await;
        let raw = upstream.receive().await.unwrap();
        finish_send(pair.wire.prepare_raw(raw, deadline).unwrap()).await;
        assert!(
            matches!(client_receive(&mut pair.client, &codec).await.remove(0), packets::Packet::SetTime(value) if value.time.0 == 12)
        );
        assert!(
            !upstream.release_received().unwrap(),
            "ordinary actual output became a terminal"
        );
        upstream.begin_close(deadline).finish_close().await.unwrap();
        assert!(
            pair.finish().await,
            "original signed guest owners did not join"
        );
    };
    tokio::join!(bds, relay);
    listener.close().await.unwrap();
    drop(listener);
    if let Some(http) = http {
        Arc::try_unwrap(http)
            .ok()
            .expect("original upstream HTTP owner leaked")
            .shutdown()
            .await;
    }
}

/// Drives only the original accepting prepared-send protocol, without changing authority policy.
async fn finish_send(mut sending: NativeAcceptingSend<'_>) {
    while let Some(capacity) = sending.next().await.unwrap() {
        capacity.commit().unwrap();
    }
    sending.finish().unwrap();
}

/// Sends generated client entries through the actual original association and codec.
async fn client_send<S: bedrock_net::BatchPhase>(
    client: &nethernet_tokio::NetherClient,
    codec: &BatchCodec<S>,
    packets: Vec<packets::Packet>,
) {
    for packet in packets {
        for message in codec.encode(&[raw_packet(packet)]).unwrap() {
            client.send(message).await.unwrap();
        }
    }
}

/// Decodes only an actual client-side reliable frame with the same directional packet codec.
async fn client_receive<S: bedrock_net::BatchPhase>(
    client: &mut nethernet_tokio::NetherClient,
    codec: &BatchCodec<S>,
) -> Vec<packets::Packet> {
    let DecodeOutcome::Packets { packets, .. } =
        codec.decode(client.recv().await.unwrap().unwrap()).unwrap()
    else {
        panic!("actual relay sent no packets")
    };
    packets
        .map(|raw| {
            let mut reader = wire::Reader::from_shared(&raw);
            let header = wire::PacketHeader::from_raw(reader.read_var_u32().unwrap());
            crate::codec::decode_body(
                packets::PacketId::from_raw(header.id).unwrap(),
                &raw.slice(reader.position()..),
            )
            .unwrap()
        })
        .collect()
}

/// Restores one original generated frame without inventing another packet algorithm.
fn raw_packet(packet: packets::Packet) -> Bytes {
    let body = crate::session_transport::batch_body(&[crate::Packet::new(packet)]).unwrap();
    let frame = wire::Frames::new(body, 1).next().unwrap().unwrap();
    let mut reader = wire::Reader::from_shared(frame.frame());
    reader.read_var_u32().unwrap();
    frame.frame().slice(reader.position()..)
}

/// Observes offline claims only after the real Login has reached the selected BDS-side socket.
fn assert_offline_name(login: &packets::Login) {
    let bytes = &login.connection_request;
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let envelope: Value = serde_json::from_slice(&bytes[4..4 + length]).unwrap();
    let claims = URL_SAFE_NO_PAD
        .decode(
            envelope["Token"]
                .as_str()
                .unwrap()
                .split('.')
                .nth(1)
                .unwrap(),
        )
        .unwrap();
    let claims: Value = serde_json::from_slice(&claims).unwrap();
    assert_eq!(claims["xname"], "Synthetic");
    assert_eq!(claims["xid"], "");
    assert_eq!(claims["mid"], "");
    assert_eq!(claims["tid"], "");
    let offset = 4 + length;
    let size = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let client = std::str::from_utf8(&bytes[offset + 4..offset + 4 + size]).unwrap();
    let payload = URL_SAFE_NO_PAD
        .decode(client.split('.').nth(1).unwrap())
        .unwrap();
    let payload = std::str::from_utf8(&payload).unwrap();
    assert!(
        payload.contains("\"opaque\":1e1"),
        "original client-data lexeme was normalized"
    );
    let client: Value = serde_json::from_str(payload).unwrap();
    assert_eq!(
        client["ThirdPartyName"], "untrusted name",
        "client data is preserved rather than treated as provider authority"
    );
}

/// Exchanges a real required archive through the public client/server pack continuations and sockets.
async fn guest_packs<S: bedrock_net::BatchPhase>(
    ready: ServerPacksReady<u8>,
    mut client_login: ClientLogin,
    pair: &mut Pair,
    codec: &BatchCodec<S>,
    source: (
        &NativeHostedUpstream,
        bedrock_client::server::session::ServerPackSnapshot,
    ),
    provider: &NativeTokenVerifier,
    clock: (std::time::SystemTime, Instant, bool, u8),
) -> ServerWorldReady<u8> {
    let Pair { wire, client, .. } = pair;
    let (upstream, snapshot) = source;
    let (utc, deadline, premium, pack_type) = clock;
    let (mut server, initial) = ready
        .start_packs(
            snapshot,
            ServerPackDelivery::default(),
            provider,
            Instant::now(),
            utc,
        )
        .unwrap();
    finish_send(wire.prepare_packets(initial, deadline).unwrap()).await;
    let mut client_packs = None;
    for packet in client_receive(client, codec).await {
        match client_login.receive(packet).unwrap() {
            LoginEvent::Send { login, packets } => {
                client_login = login;
                for packet in packets {
                    assert!(matches!(packet, packets::Packet::ClientCacheStatus(_)));
                    client_send(client, codec, vec![packet]).await;
                    let ServerPackEvent::Send { session, packets } = server
                        .receive(wire.receive(deadline).await.unwrap(), Instant::now())
                        .unwrap()
                    else {
                        panic!("actual cache response changed pack ownership")
                    };
                    assert!(packets.is_empty());
                    server = session;
                }
            }
            LoginEvent::Packs {
                cache_status,
                packs,
            } => {
                if let Some(status) = cache_status {
                    client_send(client, codec, vec![status.into()]).await;
                    let ServerPackEvent::Send { session, packets } = server
                        .receive(wire.receive(deadline).await.unwrap(), Instant::now())
                        .unwrap()
                    else {
                        panic!("client cache status changed pack ownership")
                    };
                    assert!(packets.is_empty());
                    server = session;
                }
                let (packs, response) = packs.start().unwrap();
                client_packs = Some(packs);
                client_send(client, codec, vec![response.into()]).await;
                break;
            }
            _ => panic!("unexpected real client Login transition"),
        }
    }
    let packs = client_packs.expect("actual offer did not reach guest");
    let ServerPackEvent::Send { session, packets } = server
        .receive(wire.receive(deadline).await.unwrap(), Instant::now())
        .unwrap()
    else {
        panic!("original archive data announcement missing")
    };
    server = session;
    let packets::Packet::ResourcePackDataInfo(data) = &packets[0] else {
        panic!("original DataInfo missing")
    };
    if premium {
        assert!(data.is_premium_pack);
    } else {
        assert!(!data.is_premium_pack);
    }
    assert_eq!(data.pack_type.0, pack_type);
    let expected_hash: [u8; 32] =
        Sha256::digest(upstream.read_pack(0, 0, data.file_size.0 as usize).unwrap()).into();
    assert_eq!(data.file_hash.as_ref(), expected_hash.as_slice());
    finish_send(wire.prepare_packets(packets, deadline).unwrap()).await;
    let PacksEvent::Send { packs, packets } = packs
        .receive(client_receive(client, codec).await.remove(0))
        .unwrap()
    else {
        panic!("guest did not request original archive chunk")
    };
    client_send(client, codec, packets).await;
    let ServerPackEvent::ReadChunk(read) = server
        .receive(wire.receive(deadline).await.unwrap(), Instant::now())
        .unwrap()
    else {
        panic!("original external source lease was bypassed")
    };
    let bytes = upstream
        .read_pack(read.archive_index(), read.offset(), read.byte_len())
        .unwrap();
    let chunk = read.supply(bytes.to_vec(), Instant::now()).unwrap();
    finish_send(
        wire.prepare_packets(vec![chunk.packet().clone().into()], deadline)
            .unwrap(),
    )
    .await;
    let (session, packets) = chunk.flushed(Instant::now()).unwrap();
    server = session;
    assert!(packets.is_empty());
    let PacksEvent::Send { packs, packets } = packs
        .receive(client_receive(client, codec).await.remove(0))
        .unwrap()
    else {
        panic!("guest did not verify original archive completion")
    };
    client_send(client, codec, packets).await;
    let ServerPackEvent::Send { session, packets } = server
        .receive(wire.receive(deadline).await.unwrap(), Instant::now())
        .unwrap()
    else {
        panic!("actual original stack missing")
    };
    server = session;
    finish_send(wire.prepare_packets(packets, deadline).unwrap()).await;
    let PacksEvent::Finished(complete) = packs
        .receive(client_receive(client, codec).await.remove(0))
        .unwrap()
    else {
        panic!("guest did not finish original stack")
    };
    let (response, handoff) = complete.into_parts();
    assert_eq!(handoff.into_archives().len(), 1);
    client_send(client, codec, vec![response.into()]).await;
    let ServerPackEvent::WorldReady(ready) = server
        .receive(wire.receive(deadline).await.unwrap(), Instant::now())
        .unwrap()
    else {
        panic!("actual stack acknowledgment did not reach original owner")
    };
    ready
}

/// The actual public local handoff charges and owns only its retained startup bytes.
#[tokio::test]
async fn local_bds_startup_detaches_from_a_discarded_secondary_batch_sibling() {
    tokio::time::timeout(TIMEOUT, async {
        const PADDING_BYTES: usize = 16 * 1024;
        let FixtureRoute {
            mut listener,
            target,
            http,
            approval,
            ..
        } = fixture_route(Route::LocalHttp).await;
        assert!(approval.is_none());
        let tiny = packets::SetTime {
            time: wire::ZigZag32(1),
        };
        let start = packets::StartGame {
            runtime_id: ActorRuntimeID(42),
            ..Default::default()
        };
        let after = packets::SetTime {
            time: wire::ZigZag32(2),
        };
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
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
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::Login(_)
            ));
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
                &[crate::Packet::new(packets::ResourcePackStack::default())],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                packets::Packet::ResourcePackClientResponse(_)
            ));
            let mut secondary = crate::Packet::new(packets::Transfer {
                server_address: "x".repeat(PADDING_BYTES).into(),
                server_port: wire::U16LE(0),
                ..Default::default()
            });
            secondary.header.from_subclient = 1;
            send(
                &peer,
                &codec,
                &[crate::Packet::new(tiny.clone()), secondary],
            )
            .await;
            send(
                &peer,
                &codec,
                &[
                    crate::Packet::new(start.clone()),
                    crate::Packet::new(after.clone()),
                ],
            )
            .await;
            peer.session.closed().await;
        };
        let client = async {
            let raw = RawValue::from_string(
                r#"{"ThirdPartyName":"untrusted","Nonce":"original"}"#.into(),
            )
            .unwrap();
            let request = encode_authenticated_request(
                &key(),
                &signed(claims(), "synthetic"),
                &raw,
                BatchLimits::default().max_decoded_bytes,
            )
            .unwrap();
            let login = bedrock_net::login::verify_authenticated_request(
                &request,
                &verifier(),
                UNIX_EPOCH + Duration::from_secs(SERVICE_SECONDS),
                BatchLimits::default().max_decoded_bytes,
            )
            .unwrap();
            let observations = Arc::new(std::sync::Mutex::new(Vec::new()));
            let deadline = Instant::now() + TIMEOUT;
            let mut upstream = crate::login::native_network::connection::OBSERVE_BATCHES
                .scope(
                    Arc::clone(&observations),
                    NativeHostedUpstream::connect(&target, &login, false, deadline),
                )
                .await
                .unwrap();
            let large = {
                let observed = observations.lock().unwrap();
                assert_eq!(
                    observed
                        .iter()
                        .filter(|(size, _)| *size > PADDING_BYTES)
                        .count(),
                    1,
                    "fixture must observe exactly one original larger socket batch"
                );
                observed
                    .iter()
                    .find(|(size, _)| *size > PADDING_BYTES)
                    .unwrap()
                    .1
                    .clone()
            };
            // The actual tiny startup remains in the original FIFO while its discarded sibling retires.
            let released_at_handoff = large.upgrade().is_none();
            for expected in [
                raw_packet(tiny.clone().into()),
                raw_packet(start.clone().into()),
                raw_packet(after.clone().into()),
            ] {
                assert_eq!(
                    upstream.receive().await.unwrap(),
                    expected,
                    "storage detachment changed original raw startup or unread sibling order"
                );
            }
            let mut closing = upstream.begin_close(deadline);
            closing.finish_close().await.unwrap();
            drop(closing);
            assert!(
                large.upgrade().is_none(),
                "original consumed transport input did not retire"
            );
            released_at_handoff
        };
        let (_, released_at_handoff) = tokio::join!(server, client);
        listener.close().await.unwrap();
        drop(listener);
        Arc::try_unwrap(http.unwrap())
            .ok()
            .expect("original HTTP listener leaked")
            .shutdown()
            .await;
        assert!(
            released_at_handoff,
            "tiny retained BDS startup pinned the discarded secondary batch backing"
        );
    })
    .await
    .expect("finite actual BDS startup backing fixture stalled");
}
