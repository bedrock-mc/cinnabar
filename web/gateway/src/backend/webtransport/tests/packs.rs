//! The optional owned BDS fixture crosses the original relay pump and actual server pack uni.

use super::*;
use protocol::{
    ClientRandomId, LoginSequence, LoginSettings, NativeJoinPreparation, SessionIo, SessionLogin,
    WorldEvent,
};
use std::net::SocketAddr;

/// Qualifies actual join/hash/archive/FIN through shared web startup; missing external BDS is named.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_play_reaches_owned_bds_with_its_original_pack_uni() {
    let address = match std::env::var("CINNABAR_GATEWAY_BDS_ADDRESS") {
        Ok(address) => address
            .parse::<SocketAddr>()
            .expect("numeric BDS fixture socket"),
        Err(std::env::VarError::NotPresent) => {
            eprintln!(
                "skipping paired_play_reaches_owned_bds_with_its_original_pack_uni: missing fixture CINNABAR_GATEWAY_BDS_ADDRESS"
            );
            return;
        }
        Err(error) => panic!("invalid BDS fixture input: {error}"),
    };
    assert!(address.ip().is_loopback() && address.port() != 0);
    tokio::time::timeout(
        protocol::NATIVE_JOIN_TIMEOUT + Duration::from_secs(60),
        async {
            let mut fixture = Fixture::new(Some(address)).await;
            let mut peer = fixture.peer(RELAY_TRANSPORT_PATH, None).await;
            assert_eq!(peer.status().await, StatusCode::OK);
            let (mut control, capability) = control(&peer).await;
            let join = 1;
            let mut play = Framed::new(
                peer.stream().await,
                bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES),
            );
            play.send(hello(RELAY_CHANNEL_PLAY, capability.generation, join))
                .await
                .unwrap();
            let (read, write) = tokio::io::split(play.into_inner());
            let (reader, frames, writer) = bridge::tasks::prepare_bounded_connection(
                read,
                write,
                bridge::MAX_FRAME_LEN,
                RELAY_QUEUED_FRAME_BYTES,
            );
            let settings = LoginSettings {
                view_distance_chunks: 4,
                ..LoginSettings::default()
            };
            let world = async {
                let (mut session, (required, archives)) = LoginSequence::connect_web_prepared(
                    SessionIo {
                        reader,
                        frames,
                        timers: bridge::tasks::Timers::tokio(),
                    },
                    SessionLogin {
                        display_name: "GatewayFixture",
                        client_random_id: ClientRandomId::new(1).unwrap(),
                        cache: None,
                        skin: None,
                        settings: &settings,
                        tap: None,
                        #[cfg(not(target_arch = "wasm32"))]
                        relay: None,
                        target: Some(bridge::ConnectTarget::RakNet(address.to_string())),
                    },
                    join,
                    async { Ok(Some(peer.pack_stream().await)) },
                    &NativeJoinPreparation,
                    |packs, _game| (packs.required(), packs.into_archives()),
                )
                .await
                .expect("actual paired gateway and pack uni reach shared SpawnReady");
                assert!(required);
                assert_eq!(archives.len(), 1, "original required BDS archive");
                assert!(
                    !archives[0].archive.is_empty(),
                    "original verified archive bytes"
                );
                session.finish_loading().await.unwrap();
                loop {
                    if matches!(
                        session.recv_world_event(0).await.unwrap(),
                        WorldEvent::LevelChunk(_)
                    ) {
                        break;
                    }
                }
                session.close().await.unwrap();
            };
            tokio::pin!(world, writer);
            tokio::select! { biased;
                () = &mut world => {},
                () = &mut writer => panic!("actual play writer ended before startup/terrain/close"),
            }
            writer.await;
            let attachment = fixture
                .registry
                .open_play(&capability.resume, capability.generation, Instant::now())
                .unwrap();
            let next = attachment
                .session
                .begin_play(attachment.generation, join + 1)
                .await
                .expect("original play retired before the next join owns its account");
            next.current().unwrap();
            drop(next);
            drop(attachment);
            control
                .send(Bytes::from(
                    serde_json::to_vec(&ControlRequest::SignOut).unwrap(),
                ))
                .await
                .unwrap();
            loop {
                match event(&mut control).await {
                    Event::Account { .. } => break,
                    Event::Connect { .. } => {}
                    other => panic!("same original controller after real play: {other:?}"),
                }
            }
            drop(control);
            peer.close();
            fixture.finish().await;
        },
    )
    .await
    .expect("actual paired gateway BDS world/pack fixture stalled");
}

mod blocked;
