//! Web pack delivery composes with host-owned play I/O and startup execution.

use super::*;
use bridge::FrameRead;
use futures::{FutureExt, Stream};
use sha2::{Digest, Sha256};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

const JOIN: u64 = 9;

/// Declares a required streamed archive with the same startup packets as native joins.
fn web_metadata(bulk: bool) -> serde_json::Value {
    serde_json::json!({
        "identity": identity("Fixture"), "client_cache": false, "packs_required": true,
        "trace_conn": 91, "pack_stream": bulk.then_some(JOIN),
        "packs": [{"uuid": "00112233-4455-6677-8899-aabbccddeeff", "version": "1.0.0",
            "sub_pack": "high", "content_key": "key", "size": 3,
            "sha256": <[u8; 32]>::from(Sha256::digest(b"abc"))}]
    })
}

/// Prefixes the independent archive stream with its join identity and bridge frame length.
fn archive_wire(data: &[u8]) -> Vec<u8> {
    let frame = pack_frame(0, data);
    let mut wire = JOIN.to_be_bytes().to_vec();
    wire.extend_from_slice(&(frame.len() as u32).to_be_bytes());
    wire.extend_from_slice(&frame);
    wire
}

/// Supplies addressed login choices without filesystem cache access.
fn web_login(settings: &LoginSettings) -> SessionLogin<'_> {
    SessionLogin {
        display_name: "Fixture",
        client_random_id: ClientRandomId::new(-7).unwrap(),
        cache: None,
        skin: None,
        settings,
        tap: None,
        #[cfg(not(target_arch = "wasm32"))]
        relay: None,
        target: Some(bridge::ConnectTarget::RakNet(
            "game.example.test:19132".into(),
        )),
    }
}

/// A local host reader whose trace registration is observable without global tracing.
struct HostFrames<R> {
    reader: R,
    trace: Rc<Cell<u64>>,
}

impl<R: FrameRead> Stream for HostFrames<R> {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.reader).poll_next(cx)
    }
}

impl<R: FrameRead> FrameRead for HostFrames<R> {
    fn trace_conn(&self) -> u64 {
        self.trace.get()
    }

    fn set_trace_conn(&self, id: u64) {
        self.trace.set(id);
        self.reader.set_trace_conn(id);
    }
}

#[tokio::test]
async fn injected_web_join_verifies_packs_and_preserves_play_order_and_trace() {
    for bulk in [true, false] {
        let (client, peer) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(client);
        let (reader, frames, writer) =
            bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, 4096);
        let writer = tokio::spawn(writer);
        let trace = Rc::new(Cell::new(0));
        let reader = HostFrames {
            reader,
            trace: Rc::clone(&trace),
        };
        let jobs = Rc::new(Cell::new(0));
        let preparation = LocalPreparation(Rc::clone(&jobs));
        let settings = LoginSettings {
            view_distance_chunks: 23,
            ..Default::default()
        };
        let wire = archive_wire(b"abc");
        let join = LoginSequence::connect_web_prepared(
            SessionIo {
                reader,
                frames,
                timers: bridge::tasks::Timers::tokio(),
            },
            web_login(&settings),
            JOIN,
            async { Ok(bulk.then_some(wire.as_slice())) },
            &preparation,
            keep_prepared,
        );
        let core = async {
            let mut core = FakeCore { stream: peer };
            let connect = core.receive().await;
            let request: ConnectRequest = serde_json::from_slice(&connect[1..]).unwrap();
            assert_eq!(request.target, web_login(&settings).target);
            assert_eq!(request.protocol, PROTOCOL_VERSION);
            core.send(handoff_frame(
                web_metadata(bulk),
                &[
                    Packet::new(SetTime {
                        time: wire::ZigZag32(1),
                    }),
                    start_game(),
                ],
            ))
            .await;
            if !bulk {
                core.send(pack_frame(0, b"abc")).await;
            }
            core.send(batch_frame(&[Packet::new(SetTime {
                time: wire::ZigZag32(2),
            })]))
            .await;
            core.send(batch_frame(&spawn_prerequisites())).await;
            core
        };
        let (joined, mut core) = tokio::join!(join, core);
        let (mut session, (packs, data)) = joined.expect("verified web archives reach play");
        assert_eq!(
            jobs.get(),
            1,
            "the host prepares startup after web admission"
        );
        assert_eq!(data.world.local_player_runtime_id, RUNTIME_ID);
        assert_eq!(
            trace.get(),
            91,
            "the underlying host receives the handoff identity"
        );
        assert!(packs.required());
        let archives = packs.into_archives();
        assert_eq!(archives.len(), 1);
        assert_eq!(archives[0].archive, b"abc");
        assert_eq!(archives[0].sub_pack_name, "high");
        assert_eq!(archives[0].content_key.expose(), b"key");
        for (expected_time, expected_frame) in [(1, 0), (2, 1)] {
            let crate::session_transport::Inbound::Packet(raw) =
                session.transport.recv().await.unwrap()
            else {
                panic!("queued gameplay precedes terminal messages");
            };
            let packets::Packet::SetTime(time) = *raw.decode().unwrap().data else {
                panic!("deferred time packet remains in order");
            };
            assert_eq!(time.time.0, expected_time);
            let (trace, frame, _) = session.transport.trace_position();
            assert_eq!((trace, frame), (91, expected_frame));
        }
        assert!(
            core.receive_packets().await.iter().any(
                |packet| matches!(&*packet.data, packets::Packet::RequestChunkRadius(request)
                    if request.chunk_radius.0 == i32::from(settings.view_distance_chunks))
            ),
            "web login retains the selected view distance"
        );
        session.finish_loading().await.unwrap();
        assert!(core.receive_packets().await.iter().any(|packet| matches!(
            &*packet.data,
            packets::Packet::SetLocalPlayerAsInitialized(_)
        )));
        drop(session);
        writer.await.unwrap();
    }
}

#[tokio::test]
async fn injected_web_join_rejects_bad_admission_and_digest_before_preparation() {
    for (bad_admission, expected) in [
        (true, "web archive requires a streamed digest"),
        (false, "streamed archive hash mismatch"),
    ] {
        let (client, peer) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(client);
        let (reader, frames, writer) =
            bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, 4096);
        let writer = tokio::spawn(writer);
        let jobs = Rc::new(Cell::new(0));
        let preparation = LocalPreparation(Rc::clone(&jobs));
        let settings = LoginSettings::default();
        let opened = Cell::new(false);
        let wire = archive_wire(b"abd");
        let join = LoginSequence::connect_web_prepared(
            SessionIo {
                reader,
                frames,
                timers: bridge::tasks::Timers(|_| Box::pin(std::future::ready(()))),
            },
            web_login(&settings),
            JOIN,
            async {
                opened.set(true);
                Ok(Some(wire.as_slice()))
            },
            &preparation,
            keep_prepared,
        );
        let core = async {
            let mut core = FakeCore { stream: peer };
            core.receive().await;
            let mut metadata = web_metadata(true);
            if bad_admission {
                metadata["packs"][0]["sha256"] = serde_json::Value::Null;
            }
            core.send(handoff_frame(metadata, &[start_game()])).await;
            core.send(batch_frame(&spawn_prerequisites())).await;
            core
        };
        let (result, _core) = tokio::join!(join, core);
        let error = result.err().expect("invalid archives must end the join");
        assert!(error.to_string().contains(expected), "{error}");
        assert_eq!(opened.get(), !bad_admission);
        assert_eq!(
            jobs.get(),
            0,
            "invalid archives never reach startup preparation"
        );
        writer.await.unwrap();
    }
}

/// Records release of the independent stream future when a join ends.
struct Release(Rc<Cell<bool>>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

#[tokio::test]
async fn injected_web_join_cancels_pack_open_on_terminal_play_or_drop() {
    for terminal in [true, false] {
        let (client, peer) = tokio::io::duplex(8192);
        let (read, write) = tokio::io::split(client);
        let (reader, frames, writer) =
            bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, 4096);
        let writer = tokio::spawn(writer);
        let released = Rc::new(Cell::new(false));
        let release = Release(Rc::clone(&released));
        let jobs = Rc::new(Cell::new(0));
        let preparation = LocalPreparation(Rc::clone(&jobs));
        let settings = LoginSettings::default();
        let packs = async move {
            let _release = release;
            futures::future::pending::<()>().await;
            Ok(Some(tokio::io::empty()))
        };
        let join = LoginSequence::connect_web_prepared(
            SessionIo {
                reader,
                frames,
                timers: bridge::tasks::Timers::tokio(),
            },
            web_login(&settings),
            JOIN,
            packs,
            &preparation,
            keep_prepared,
        );
        let core = async {
            let mut core = FakeCore { stream: peer };
            core.receive().await;
            core.send(handoff_frame(web_metadata(true), &[start_game()]))
                .await;
            if terminal {
                core.send(
                    crate::session_wire::encode_core_message(&CoreMessage::Disconnect(
                        crate::session_wire::SessionDisconnect {
                            reason: 0,
                            message: "closed".into(),
                            filtered_message: String::new(),
                            hide_screen: false,
                        },
                    ))
                    .unwrap(),
                )
                .await;
            }
            core
        };
        if terminal {
            let (result, _core) = tokio::join!(join, core);
            assert!(result.err().unwrap().server_disconnect().is_some());
        } else {
            let mut join = Box::pin(join);
            let _core = tokio::select! {
                core = core => core,
                _ = &mut join => panic!("join waits for the pending archive stream"),
            };
            assert!(join.as_mut().now_or_never().is_none());
            drop(join);
        }
        assert!(
            released.get(),
            "ending the join releases pending bulk ownership"
        );
        assert_eq!(jobs.get(), 0);
        writer.await.unwrap();
    }
}
