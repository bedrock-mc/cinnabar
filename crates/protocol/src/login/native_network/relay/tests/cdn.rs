//! CDN-only acquisition through the original native Login, pack response and relay handoff owners.

use super::*;
use crate::login::native_network::packs::PUBLIC_CDN_ENDPOINT;
use bedrock_protocol_1_26_51::types::{
    PackInfoData, PackInstanceId, ResourcePackClientResponseData,
};
use resource_pack::test_support::archive_bytes;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn relay_acquires_cdn_only_archive_without_requesting_any_game_chunks() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = http.local_addr().unwrap();
        let id = uuid::Uuid::from_u128(711);
        let archive = archive_bytes(id, &[]);
        let mut offer = PackInfoData::default();
        offer.pack_id_version.pack_uuid = id;
        offer.pack_id_version.pack_version.version = "1.0.0".into();
        offer.pack_size = wire::U64LE(archive.len() as u64);
        offer.cdn_url = format!("http://{endpoint}/only-archive").into();
        let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let signaling = LanSignaling::with_config(
            49,
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
        data.connection_type = transport::LAN_CONNECTION_TYPE;
        data.protocol_version = crate::PROTOCOL_VERSION as u32;
        signaling.set_server_data(data);
        let mut listener = NetherServer::bind_with(
            signaling,
            ConnectionConfig {
                allow_unsigned_answers: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let download = async {
            let (mut socket, _) = http.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await.unwrap());
                assert!(header.len() < 8192);
            }
            assert!(header.starts_with(b"GET /only-archive HTTP/1.1\r\n"));
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        archive.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&archive).await.unwrap();
        };
        let server = async {
            let mut peer = listener.accept().await.unwrap();
            let codec = BatchCodec::new(BatchHeader::NetherNet, BatchLimits::default());
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::RequestNetworkSettings(_)
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
                Packet::Login(_)
            ));
            send(
                &peer,
                &codec,
                &[
                    crate::Packet::new(packets::PlayStatus {
                        status: PlayStatusType::Loginsuccess,
                    }),
                    crate::Packet::new(packets::ResourcePacksInfo {
                        resource_packs: vec![offer],
                        ..Default::default()
                    }),
                ],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::ClientCacheStatus(_)
            ));
            let response = receive(&mut peer, &codec).await;
            assert_eq!(
                response.len(),
                1,
                "no in-band pack requests accompany completion"
            );
            assert!(
                matches!(
                    &*response[0].data,
                    Packet::ResourcePackClientResponse(packets::ResourcePackClientResponse {
                        response: ResourcePackClientResponseData::DownloadingFinished { .. },
                    })
                ),
                "CDN-only peer never sends DataInfo or archive chunks"
            );
            send(
                &peer,
                &codec,
                &[
                    crate::Packet::new(packets::ResourcePackStack {
                        texture_pack_list: vec![PackInstanceId {
                            pack_id: id.to_string().into(),
                            version: "1.0.0".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    crate::Packet::new(packets::StartGame {
                        runtime_id: ActorRuntimeID(42),
                        ..Default::default()
                    }),
                ],
            )
            .await;
            assert!(matches!(
                &*receive(&mut peer, &codec).await[0].data,
                Packet::ResourcePackClientResponse(packets::ResourcePackClientResponse {
                    response: ResourcePackClientResponseData::ResourcePackStackFinished { .. },
                })
            ));
            peer.session.closed().await;
        };
        let client = async {
            let target =
                NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan);
            let connected = transport::Transport::connect(
                &target,
                None,
                None,
                tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT,
            )
            .await
            .unwrap();
            let mut joining =
                NativeRelayConnecting::from_connection(Connection::new(connected), target.address);
            let claims = crate::login_client_data(
                "Fixture",
                crate::ClientRandomId::new(1).unwrap(),
                None,
                &crate::LoginSettings::default(),
            );
            let claims = crate::RawClientData::from_value(&claims).unwrap();
            PUBLIC_CDN_ENDPOINT
                .scope(endpoint, joining.negotiate(claims, false, None))
                .await
                .unwrap();
            let (connected, handoff) = joining
                .try_finish()
                .unwrap_or_else(|_| panic!("actual StartGame completes original relay"));
            assert_eq!(handoff.archives.as_slice(), std::slice::from_ref(&archive));
            assert_eq!(handoff.handoff.packs.len(), 1);
            assert_eq!(handoff.handoff.packs[0].uuid, id.to_string());
            assert_eq!(
                handoff.handoff.packs[0].sha256,
                Some(Sha256::digest(&archive).into())
            );
            assert!(handoff.handoff.packs[0].cache.is_none());
            let mut closing = connected.begin_close();
            closing.finish_close().await.unwrap();
        };
        tokio::join!(download, server, client);
        listener.close().await.unwrap();
    })
    .await
    .expect("finite CDN-only relay fixture stalled");
}
