//! Accepted gateway play and the shared web consumer against an explicitly owned local BDS fixture.

use super::*;
use protocol::{
    ClientRandomId, LoginSequence, LoginSettings, NativeJoinPreparation, SessionIo, SessionLogin,
    WorldEvent,
};

/// Crosses real TLS/control/play, native login, verified archives and shared client world startup.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn accepted_play_reaches_owned_bds_world() {
    let address = match std::env::var("CINNABAR_GATEWAY_BDS_ADDRESS") {
        Ok(address) => address
            .parse::<SocketAddr>()
            .expect("numeric BDS fixture socket"),
        Err(std::env::VarError::NotPresent) => {
            eprintln!(
                "skipping accepted_play_reaches_owned_bds_world: missing fixture CINNABAR_GATEWAY_BDS_ADDRESS"
            );
            return;
        }
        Err(error) => panic!("invalid BDS fixture input: {error}"),
    };
    assert!(address.ip().is_loopback() && address.port() != 0);
    tokio::time::timeout(protocol::NATIVE_JOIN_TIMEOUT + Duration::from_secs(60), async {
        let mut fixture = Fixture::with_upstream(Some(address)).await;
        let (mut control, capability) = fixture.control(String::new()).await;
        unscoped_loopback_is_refused(&fixture, &capability, address).await;

        let join = 2;
        let uri = format!("wss://{}{RELAY_PLAY_PATH}", fixture.address)
            .parse()
            .unwrap();
        let builder = ClientBuilder::from_uri(uri)
            .limits(tokio_websockets::Limits::default().max_payload_len(Some(RELAY_PLAY_MESSAGE_BYTES)))
            .add_header(
                SEC_WEBSOCKET_PROTOCOL,
                HeaderValue::from_static(RELAY_SOCKET_PROTOCOL),
            )
            .unwrap()
            .add_header(
                ORIGIN,
                HeaderValue::from_str(&format!("https://{}", fixture.address)).unwrap(),
            )
            .unwrap();
        let (socket, response) = builder.connect_on(fixture.secure().await).await.unwrap();
        assert_eq!(
            response.headers().get(SEC_WEBSOCKET_PROTOCOL).unwrap(),
            RELAY_SOCKET_PROTOCOL
        );
        let mut play = Framed::new(
            Socket::new(socket, RELAY_PLAY_MESSAGE_BYTES),
            bridge::BridgeCodec::with_max(RELAY_CONTROL_BYTES),
        );
        play.send(Bytes::from(
            serde_json::to_vec(&Hello {
                version: RELAY_VERSION,
                channel: RELAY_CHANNEL_PLAY.into(),
                channels: vec![RELAY_CHANNEL_CONTROL.into(), RELAY_CHANNEL_PLAY.into()],
                resume: capability.resume.clone(),
                generation: capability.generation,
                join,
            })
            .unwrap(),
        ))
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
                async { Ok(None::<tokio::io::Empty>) },
                &NativeJoinPreparation,
                |packs, _game| (packs.required(), packs.into_archives()),
            )
            .await
            .expect("actual gateway handoff reaches shared web SpawnReady");
            assert!(required, "owned BDS fixture requires its original resource pack");
            assert_eq!(archives.len(), 1, "owned BDS original resource pack");
            assert!(!archives[0].archive.is_empty(), "verified original archive is nonempty");
            eprintln!("gateway world spawned; packs={}", archives.len());
            session.finish_loading().await.unwrap();
            tokio::time::timeout(Duration::from_secs(30), async {
                loop {
                    if matches!(session.recv_world_event(0).await.unwrap(), WorldEvent::LevelChunk(_)) {
                        break;
                    }
                }
            })
            .await
            .expect("actual gateway world terrain");
            eprintln!("gateway world terrain received");
            // Web sessions leave endpoint retirement to their host; releasing this session
            // closes its reader and queue after every awaited initialization write flushed.
            session.close().await.unwrap();
            drop(session);
        };
        tokio::pin!(world, writer);
        tokio::select! { biased;
            () = &mut world => {},
            () = &mut writer => panic!("actual play writer ended before world startup and close"),
        }
        writer.await;
        test_time::eventually("actual BDS play socket retired", || {
            crate::registry::tests::retained_counts(&fixture.registry) == (1, 1)
        });
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
        eprintln!("gateway normal play close complete");

        control
            .send(Bytes::from(serde_json::to_vec(&Request::SignOut).unwrap()))
            .await
            .unwrap();
        loop {
            match event(&mut control).await {
                Event::Account { .. } => break,
                Event::Connect { .. } => {},
                other => panic!("original control unusable after play: {other:?}"),
            }
        }
        control.close().await.unwrap();
        drop(control);
        fixture.finish().await;
        eprintln!("gateway original control and registry retired");
    })
    .await
    .expect("actual accepted BDS world fixture stalled");
}

/// The same real control/lease cannot connect to loopback outside the serving task's exact scope.
async fn unscoped_loopback_is_refused(
    fixture: &Fixture,
    capability: &bridge::relay_control::Capability,
    address: SocketAddr,
) {
    let attachment = fixture
        .registry
        .open_play(&capability.resume, capability.generation, Instant::now())
        .unwrap();
    let lease = attachment
        .session
        .begin_play(attachment.generation, 1)
        .await
        .unwrap();
    let result = crate::upstream::connect(
        &lease,
        Some(bridge::ConnectTarget::RakNet(address.to_string())),
        Instant::now()
            .checked_add(protocol::NATIVE_JOIN_TIMEOUT)
            .unwrap(),
    )
    .await;
    drop(lease);
    drop(attachment);
    match result {
        Err(error) => assert_eq!(error.kind(), io::ErrorKind::PermissionDenied),
        Ok(connection) => {
            let mut closing = connection.begin_close();
            let _ = closing.finish_close().await;
            panic!("unscoped loopback fixture bypassed public egress policy");
        }
    }
}
