//! Prepared play I/O joins through the same handoff, spawn and terminal sequence.
use super::*;
use std::{cell::Cell, rc::Rc};

/// Supplies queued startup frames and pauses until the test inserts later play frames.
struct QueuedFrames(futures::channel::mpsc::UnboundedReceiver<Bytes>);

impl futures::Stream for QueuedFrames {
    type Item = Result<Bytes, BridgeError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::pin::Pin::new(&mut self.0)
            .poll_next(cx)
            .map(|frame| frame.map(Ok))
    }
}

impl bridge::FrameRead for QueuedFrames {
    fn trace_conn(&self) -> u64 {
        0
    }

    fn set_trace_conn(&self, _: u64) {}
}

/// Records startup submissions without requiring a thread-safe executor handle.
struct LocalPreparation(Rc<Cell<usize>>);
impl JoinPreparation for LocalPreparation {
    type Task<T: Send + 'static> = std::future::Ready<T>;

    fn submit<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Self::Task<T> {
        self.0.set(self.0.get() + 1);
        std::future::ready(work())
    }
}

/// A radius reply received after login reaches world ingress without initializing early or twice.
#[tokio::test]
async fn prepared_io_accepts_a_late_radius_reply_after_spawn_without_radius_reply() {
    use futures::FutureExt;

    let (incoming, reader) = futures::channel::mpsc::unbounded();
    for frame in [
        handoff_frame(
            serde_json::json!({"identity": identity("Fixture"),
                "client_cache": false, "packs_required": false, "packs": []}),
            &[start_game()],
        ),
        batch_frame(&[
            Packet::new(ItemRegistry::default()),
            Packet::new(PlayStatus {
                status: PlayStatusType::Playerspawn,
            }),
        ]),
    ] {
        incoming.unbounded_send(frame).unwrap();
    }
    let (output, peer) = tokio::io::duplex(8192);
    let (_, frames, writer) =
        bridge::tasks::prepare_connection(tokio::io::empty(), output, bridge::MAX_FRAME_LEN);
    let writer = tokio::spawn(writer);
    let preparation = LocalPreparation(Rc::new(Cell::new(0)));
    let settings = LoginSettings::default();
    let join = LoginSequence::connect_prepared(
        SessionIo {
            reader: QueuedFrames(reader),
            frames,
            timers: bridge::tasks::Timers::tokio(),
        },
        SessionLogin {
            display_name: "Fixture",
            client_random_id: ClientRandomId::new(-7).unwrap(),
            cache: None,
            skin: None,
            settings: &settings,
            tap: None,
            target: None,
        },
        &preparation,
        keep_prepared,
    );
    let core = async {
        let mut core = FakeCore { stream: peer };
        assert_eq!(core.receive().await[0], bridge::contract::KIND_CONNECT);
        assert_eq!(
            core.receive_packets().await,
            [
                crate::request_chunk_radius_packet(
                    settings.view_distance_chunks,
                    render_api::MAX_VIEW_RADIUS_CHUNKS as u8,
                ),
                crate::loading_screen_packet(crate::LoadingScreenPhase::Start, None),
            ]
        );
        core
    };
    let (joined, mut core) = tokio::join!(join, core);
    let (mut session, (_, data)) =
        joined.expect("login completes before the server sends a radius reply");
    assert_eq!(data.world.local_player_runtime_id, RUNTIME_ID);
    assert!(!session.terrain_before_spawn());
    incoming
        .unbounded_send(batch_frame(&[Packet::new(ChunkRadiusUpdated {
            chunk_radius: wire::ZigZag32(11),
        })]))
        .unwrap();
    assert_eq!(
        session.recv_world_event(0).await.unwrap(),
        crate::WorldEvent::ChunkRadiusUpdated(11)
    );

    assert!(
        core.receive().now_or_never().is_none(),
        "initialization waits for presentation"
    );
    session.finish_loading().await.unwrap();
    session.finish_loading().await.unwrap();
    drop(session);
    writer.await.unwrap();
    assert_eq!(
        core.receive_packets().await,
        [
            crate::loading_screen_packet(crate::LoadingScreenPhase::End, None),
            Packet::new(packets::SetLocalPlayerAsInitialized {
                player_id: ActorRuntimeID(RUNTIME_ID),
            }),
        ]
    );
    let mut remaining = Vec::new();
    core.stream.read_to_end(&mut remaining).await.unwrap();
    assert!(remaining.is_empty(), "initialization is sent only once");
}

#[tokio::test]
async fn prepared_io_joins_an_addressed_server_and_retains_spawn_and_transfer() {
    let (client, peer) = tokio::io::duplex(8192);
    let (read, write) = tokio::io::split(client);
    let (reader, frames, writer) =
        bridge::tasks::prepare_bounded_connection(read, write, bridge::MAX_FRAME_LEN, 4096);
    let writer = tokio::spawn(writer);
    let jobs = Rc::new(Cell::new(0));
    let preparation = LocalPreparation(Rc::clone(&jobs));
    let settings = LoginSettings {
        view_distance_chunks: 23,
        ..Default::default()
    };
    let target = bridge::ConnectTarget::RakNet("game.example.test:19132".into());
    let join = LoginSequence::connect_prepared(
        SessionIo {
            reader,
            frames,
            timers: bridge::tasks::Timers::tokio(),
        },
        SessionLogin {
            display_name: "Fixture",
            client_random_id: crate::ClientRandomId::new(-7).unwrap(),
            cache: None,
            skin: None,
            settings: &settings,
            tap: None,
            target: Some(target.clone()),
        },
        &preparation,
        keep_prepared,
    );
    let core = async {
        let mut core = FakeCore { stream: peer };
        let connect = core.receive().await;
        let request: ConnectRequest = serde_json::from_slice(&connect[1..]).unwrap();
        assert_eq!(request.target, Some(target));
        assert_eq!(request.protocol, PROTOCOL_VERSION);
        core.send(handoff_frame(
            serde_json::json!({"identity": identity("Fixture"),
            "client_cache": false, "packs_required": false, "packs": []}),
            &[start_game()],
        ))
        .await;
        assert!(
            core.receive_packets().await.iter().any(
                |packet| matches!(&*packet.data, packets::Packet::RequestChunkRadius(request)
                    if request.chunk_radius.0 == i32::from(settings.view_distance_chunks))
            ),
            "prepared login retains the selected view distance"
        );
        core.send(batch_frame(&spawn_prerequisites())).await;
        core
    };
    let (joined, mut core) = tokio::join!(join, core);
    let (mut session, (_, data)) = joined.unwrap();
    assert_eq!(
        jobs.get(),
        2,
        "archive admission and presentation use host jobs"
    );
    assert_eq!(data.world.local_player_runtime_id, RUNTIME_ID);
    session.finish_loading().await.unwrap();
    let initialized = core.receive_packets().await;
    assert_eq!(
        initialized
            .iter()
            .filter(|packet| matches!(
                &*packet.data,
                packets::Packet::SetLocalPlayerAsInitialized(_)
            ))
            .count(),
        1
    );
    let mut transfer = vec![bridge::contract::KIND_TRANSFER];
    transfer
        .extend_from_slice(br#"{"address":"next.example.test","port":19133,"reload_world":false}"#);
    core.send(Bytes::from(transfer)).await;
    session.drain_to_boundary().await;
    assert_eq!(
        session.take_server_transfer().unwrap().host,
        "next.example.test"
    );
    drop(session);
    writer.await.unwrap();
}

mod web;
