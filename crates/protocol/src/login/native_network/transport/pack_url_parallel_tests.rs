//! Explicit parallel acquisition through the original native join and ordered archive handoff.

use super::super::{
    LoginSequence, SessionLogin,
    packs::{HTTP_ATTEMPTS, MAX_CDN_DOWNLOADS},
};
use super::early_spawn_tests::{receive, send};
use super::pack_url_policy_tests::{VERSION, http_archive, offer};
use super::*;
use bedrock_net::{BatchCodec, BatchLimits, CompressionAlgorithm, CompressionSettings};
use bedrock_protocol_1_26_51::{
    enums::PlayStatusType,
    packets,
    types::{ActorRuntimeID, PackInstanceId, ResourcePackClientResponseData},
    wire,
};
use futures::{StreamExt, future::join_all, stream::FuturesUnordered};
use nethernet_tokio::{NetherServer, ServerData};
use resource_pack::test_support::archive_bytes;
use std::time::Duration;
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
};

#[tokio::test]
async fn native_parallel_pack_urls_overlap_only_up_to_the_existing_cap() {
    tokio::time::timeout(Duration::from_secs(20), parallel_urls())
        .await
        .expect("finite native parallel HTTP/archive fixture stalled");
}

/// Holds every actual response, then releases one original archive to admit the final URL.
async fn parallel_urls() {
    let ids = (0..MAX_CDN_DOWNLOADS + 1)
        .map(|index| uuid::Uuid::from_u128(201 + index as u128))
        .collect::<Vec<_>>();
    let archives = ids
        .iter()
        .map(|id| archive_bytes(*id, &[]))
        .collect::<Vec<_>>();
    let mut responses = FuturesUnordered::new();
    let mut offers = Vec::new();
    let mut ready = Vec::new();
    let mut release = Vec::new();
    for (id, archive) in ids.iter().zip(&archives) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        offers.push(offer(*id, archive, listener.local_addr().unwrap()));
        let (body_ready, reading) = oneshot::channel();
        let (write, released) = oneshot::channel();
        ready.push(reading);
        release.push(Some(write));
        responses.push(http_archive(
            listener,
            archive.clone(),
            Some((body_ready, released)),
        ));
    }
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
                        .iter()
                        .copied()
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
        let settings = crate::LoginSettings {
            parallel_resource_pack_urls: true,
            ..Default::default()
        };
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
        let last_ready = ready.pop().unwrap();
        let first_ready = join_all(ready);
        tokio::select! {
            result = &mut joining => { drop(result); panic!("real HTTP archive bodies remain held"); }
            results = first_ready => { for result in results { result.unwrap(); } }
        }
        let before_release = std::iter::from_fn(|| observed.try_recv().ok()).collect::<Vec<_>>();
        release[0].take().unwrap().send(()).unwrap();
        tokio::select! {
            result = &mut joining => { drop(result); panic!("the remaining real HTTP bodies remain held"); }
            result = last_ready => result.unwrap(),
        }
        let after_one = std::iter::from_fn(|| observed.try_recv().ok()).collect::<Vec<_>>();
        for write in release.into_iter().flatten() {
            write.send(()).unwrap();
        }
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
        all_attempts.extend(after_one.iter().copied());
        all_attempts.extend(std::iter::from_fn(|| observed.try_recv().ok()));
        (before_release, after_one, all_attempts)
    };
    let http = async { while responses.next().await.is_some() {} };
    let ((), (), (mut before_release, after_one, mut all_attempts)) =
        tokio::join!(server, http, client);
    listener.close().await.unwrap();
    before_release.sort_unstable();
    all_attempts.sort_unstable();
    assert_eq!(before_release, ids[..MAX_CDN_DOWNLOADS]);
    assert_eq!(after_one, [ids[MAX_CDN_DOWNLOADS]]);
    assert_eq!(all_attempts, ids);
}
