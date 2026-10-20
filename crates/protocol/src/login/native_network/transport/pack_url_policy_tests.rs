//! Default scheduling through genuine native Login, HTTP archives and final pack handoff.

use super::super::{LoginSequence, SessionLogin, packs::HTTP_ATTEMPTS};
use super::early_spawn_tests::{receive, send};
use super::*;
use bedrock_net::{BatchCodec, BatchLimits, CompressionAlgorithm, CompressionSettings};
use bedrock_protocol_1_26_51::{
    enums::PlayStatusType,
    packets,
    types::{ActorRuntimeID, PackInfoData, PackInstanceId, ResourcePackClientResponseData},
    wire,
};
use nethernet_tokio::{NetherServer, ServerData};
use resource_pack::test_support::archive_bytes;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
};

pub(super) const VERSION: &str = "1.0.0";
const HTTP_HEADER_LIMIT: usize = 8192;
type Hold = (oneshot::Sender<()>, oneshot::Receiver<()>);

/// Serves original archive bytes, optionally holding the real response body after its header.
pub(super) async fn http_archive(listener: TcpListener, bytes: Vec<u8>, hold: Option<Hold>) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        request.push(socket.read_u8().await.unwrap());
        assert!(request.len() < HTTP_HEADER_LIMIT);
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
    if let Some((ready, released)) = hold {
        ready.send(()).unwrap();
        released.await.unwrap();
    }
    socket.write_all(&bytes).await.unwrap();
}

/// Advertises exactly the original synthetic archive to the real pack negotiation owner.
pub(super) fn offer(id: uuid::Uuid, bytes: &[u8], address: SocketAddr) -> PackInfoData {
    let mut offer = PackInfoData::default();
    offer.pack_id_version.pack_uuid = id;
    offer.pack_id_version.pack_version.version = VERSION.into();
    offer.pack_size = wire::U64LE(bytes.len() as u64);
    offer.cdn_url = format!("http://{address}/pack").into();
    offer
}

#[tokio::test]
async fn native_default_pack_urls_wait_for_the_first_archive_before_starting_the_next() {
    tokio::time::timeout(Duration::from_secs(20), default_urls())
        .await
        .expect("finite native HTTP/archive fixture stalled");
}

/// Completes the actual join and cleanup before checking the recorded blocked-first attempts.
async fn default_urls() {
    let first_http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let second_http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ids = [uuid::Uuid::from_u128(101), uuid::Uuid::from_u128(102)];
    let archives = ids.map(|id| archive_bytes(id, &[]));
    let offers = [
        offer(ids[0], &archives[0], first_http.local_addr().unwrap()),
        offer(ids[1], &archives[1], second_http.local_addr().unwrap()),
    ];
    let (body_ready, ready) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let first_response = http_archive(
        first_http,
        archives[0].clone(),
        Some((body_ready, released)),
    );
    let second_response = http_archive(second_http, archives[1].clone(), None);

    let reservation = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let signaling = LanSignaling::with_config(
        41,
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
                crate::Packet::new(packets::ResourcePacksInfo {
                    resource_packs: offers.to_vec(),
                    ..Default::default()
                }),
            ],
        )
        .await;
        assert!(matches!(
            &*receive(&mut peer, &codec).await[0].data,
            packets::Packet::ClientCacheStatus(_)
        ));
        assert!(matches!(
            &*receive(&mut peer, &codec).await[0].data,
            packets::Packet::ResourcePackClientResponse(packets::ResourcePackClientResponse {
                response: ResourcePackClientResponseData::DownloadingFinished { .. },
            })
        ));
        send(
            &peer,
            &codec,
            &[
                crate::Packet::new(packets::PlayStatus {
                    status: PlayStatusType::Playerspawn,
                }),
                crate::Packet::new(packets::ResourcePackStack {
                    texture_pack_list: ids
                        .into_iter()
                        .map(|id| PackInstanceId {
                            pack_id: id.to_string().into(),
                            version: VERSION.into(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
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
            packets::Packet::ResourcePackClientResponse(packets::ResourcePackClientResponse {
                response: ResourcePackClientResponseData::ResourcePackStackFinished { .. },
            })
        ));
        assert!(matches!(
            &*receive(&mut peer, &codec).await[0].data,
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
        peer.session.closed().await;
    };
    let client = async {
        let settings = crate::LoginSettings::default();
        let target =
            NativeConnectionTarget::new(address.to_string(), NativeTransport::NetherNetLan);
        let (attempts, mut observed) = mpsc::channel(ids.len());
        let joining = HTTP_ATTEMPTS.scope(
            attempts,
            LoginSequence::connect_native(
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
                |packs, game| (packs.into_archives(), game.world.local_player_runtime_id),
            ),
        );
        tokio::pin!(joining);
        tokio::select! {
            result = &mut joining => { drop(result); panic!("the first real HTTP body is still held"); }
            result = ready => result.unwrap(),
        }
        let mut before_release = Vec::new();
        while let Ok(id) = observed.try_recv() {
            before_release.push(id);
        }
        release.send(()).unwrap();
        let (mut play, (received, runtime_id)) = joining.await.unwrap();
        assert_eq!(runtime_id, 42);
        assert_eq!(received.len(), archives.len());
        for (index, pack) in received.iter().enumerate() {
            assert_eq!(pack.pack_id, ids[index]);
            assert_eq!(pack.version, VERSION);
            assert_eq!(pack.archive, archives[index]);
        }
        play.finish_loading().await.unwrap();
        play.close().await.unwrap();
        let mut all_attempts = before_release.clone();
        while let Ok(id) = observed.try_recv() {
            all_attempts.push(id);
        }
        (before_release, all_attempts)
    };
    let ((), (), (), (before_release, all_attempts)) =
        tokio::join!(server, first_response, second_response, client);
    listener.close().await.unwrap();
    assert_eq!(all_attempts, ids);
    assert_eq!(
        before_release,
        [ids[0]],
        "absent explicit configuration, the first archive must complete before the next HTTP attempt"
    );
}
