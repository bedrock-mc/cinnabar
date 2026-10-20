//! Real play cancellation retires the original pack writer before its account can be reused.

use super::*;
use h3::quic::{RecvStream, StreamErrorIncoming};
use tokio::sync::oneshot;

type ArchiveStream = h3_webtransport::stream::RecvStream<h3_quinn::RecvStream, Bytes>;

/// Withholds actual uni credit, then observes the writer's reset while its controller remains live.
#[tokio::test]
async fn canceling_play_resets_its_blocked_pack_uni_before_reusing_the_account() {
    let address = match std::env::var("CINNABAR_GATEWAY_BDS_ADDRESS") {
        Ok(address) => address
            .parse::<SocketAddr>()
            .expect("numeric BDS fixture socket"),
        Err(std::env::VarError::NotPresent) => {
            eprintln!(
                "skipping canceling_play_resets_its_blocked_pack_uni_before_reusing_the_account: missing fixture CINNABAR_GATEWAY_BDS_ADDRESS"
            );
            return;
        }
        Err(error) => panic!("invalid BDS fixture input: {error}"),
    };
    assert!(address.ip().is_loopback() && address.port() != 0);
    tokio::time::timeout(
        protocol::NATIVE_JOIN_TIMEOUT + Duration::from_secs(60),
        async {
            let credit = 1_u32;
            let mut fixture = Fixture::with_pack_credit(address, credit).await;
            let mut peer = fixture.peer(RELAY_TRANSPORT_PATH, None).await;
            assert_eq!(peer.status().await, StatusCode::OK);
            let (mut control, capability) = control(&peer).await;
            let join = 1_u64;
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
            let (blocked, mut observing) = oneshot::channel();
            let receiving = async {
                // The original h3 peer has consumed the actual uni header and checked its session.
                let mut archive = peer.pack_stream().await;
                assert_eq!(archive.read_u8().await.unwrap(), join.to_be_bytes()[0]);
                assert!(blocked.send(archive).is_ok());
                std::future::pending::<Result<Option<ArchiveStream>, protocol::ProtocolError>>()
                    .await
            };
            let mut connecting = Box::pin(LoginSequence::connect_web_prepared(
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
                receiving,
                &NativeJoinPreparation,
                |packs, _game| (packs.required(), packs.into_archives()),
            ));
            let mut writer = Box::pin(writer);
            let mut archive = tokio::select! { biased;
                result = &mut observing => result.unwrap(),
                result = &mut connecting => panic!("original join ended before archive pressure: {:?}", result.err()),
                () = &mut writer => panic!("original writer ended before archive pressure"),
            };
            assert!(peer.connection.close_reason().is_none());
            // Only one payload byte was consumed. The remaining original join prefix cannot fit.
            let mut admitted = 1_usize;
            let maximum = usize::try_from(credit).unwrap() + admitted;
            assert!(maximum < join.to_be_bytes().len());
            drop(connecting);
            writer.await;
            // Account reuse waits for the actual old play/pack owners before reads grant more credit.
            let attachment = fixture
                .registry
                .open_play(&capability.resume, capability.generation, Instant::now())
                .unwrap();
            let next = attachment
                .session
                .begin_play(attachment.generation, join + 1)
                .await
                .expect("original pressured play retired before account reuse");
            next.current().unwrap();
            let reset = loop {
                match futures::future::poll_fn(|cx| archive.poll_data(cx)).await {
                    Ok(Some(bytes)) => {
                        admitted += bytes.len();
                        assert!(admitted <= maximum, "blocked writer exceeded actual uni credit");
                    }
                    Ok(None) => panic!("canceled original pack writer sent clean FIN"),
                    Err(error) => break error,
                }
            };
            assert!(matches!(
                &reset,
                StreamErrorIncoming::StreamTerminated { error_code: 0 }
            ), "pack retirement must be its original RESET, not physical connection loss: {reset:?}");
            assert!(peer.connection.close_reason().is_none());
            drop(archive);
            drop(next);
            drop(attachment);
            assert_eq!(crate::registry::tests::retained_counts(&fixture.registry), (1, 1));

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
                    other => panic!("original controller changed after canceled play: {other:?}"),
                }
            }
            drop(control);
            peer.close();
            fixture.finish().await;
        },
    )
    .await
    .expect("actual blocked pack cancellation/account fixture stalled");
}
