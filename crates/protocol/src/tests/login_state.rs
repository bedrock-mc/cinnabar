//! Joins a scripted core over the session endpoint, then checks what play makes of its traffic.

use std::time::Duration;

use bedrock_protocol_1_26_51::enums::{
    ConnectionDisconnectFailReason, PlayStatusType, ServerboundLoadingScreenType,
};
use bedrock_protocol_1_26_51::packets::{
    self, ChunkRadiusUpdated, ClientCacheBlobStatus, ClientCacheMissResponse, ClientCacheStatus,
    Disconnect, ItemRegistry, LevelChunk, NetworkStackLatency, PacketId, PlayStatus,
    ServerboundLoadingScreen, SetLocalPlayerAsInitialized, SetTime, StartGame,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, ChunkPos, DimensionType, DisconnectMessages, ItemData, MissingBlobData,
    SubChunkMetadata,
};
use bedrock_protocol_1_26_51::wire::{self, Encode};
use bridge::contract::{KIND_BATCH, KIND_CONNECT, KIND_DISCONNECT, KIND_HANDOFF, KIND_TRANSFER};
use bytes::{BufMut, Bytes, BytesMut};
use futures::{SinkExt, StreamExt};
use protocol::{
    ClientBlobCache, GameData, LoginSequence, Packet, PlaySession, ProtocolError, WorldEvent,
};
use tokio::sync::mpsc;

#[path = "login_state/camera_instructions.rs"]
mod camera_instructions;
#[path = "login_state/disconnect_reason.rs"]
mod disconnect_reason;
#[path = "login_state/item_registry.rs"]
mod item_registry;
#[path = "login_state/level_chunk_wire_failure.rs"]
mod level_chunk_wire_failure;
#[path = "login_state/modal_forms.rs"]
mod modal_forms;
#[path = "login_state/unused_store_packets.rs"]
mod unused_store_packets;
use disconnect_reason::{
    PlayEpilogue, boundary_epilogue, camera_instruction_epilogue_packets, skipped_epilogue_wire,
    truncated_epilogue_wire,
};

const RUNTIME_ID: u64 = 0x1234_5678;
const OTHER_RUNTIME_ID: u64 = 0x7654_3210;
const SCRIPTED_PLAY_END_TIME: i32 = 34_567; // The last SetTime of the default play traffic.
const CACHE_CLASSIFIED_TIME: i32 = 67_890;
const CORE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy)]
enum SpawnOrder {
    RadiusThenSpawn,
    SpawnThenRadius,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CachePlayScript {
    ResolveValid,
    TruncatedMissResponse,
    InvalidMissResponseThenTraffic,
    MalformedLevelChunk,
    TrailingLevelChunk,
}

/// What the scripted core sends around the client's spawn and play.
#[derive(Clone, Copy)]
struct Script {
    order: SpawnOrder,
    conflicting_start: bool,
    dimension_definition_before_start: bool,
    cache_enabled: bool,
    cache_play_script: CachePlayScript,
    epilogue: PlayEpilogue,
    terminal_boundary: bool, // a boundary epilogue arrives as the core's terminal message
}

impl Default for Script {
    fn default() -> Self {
        Self {
            order: SpawnOrder::RadiusThenSpawn,
            conflicting_start: false,
            dimension_definition_before_start: false,
            cache_enabled: false,
            cache_play_script: CachePlayScript::ResolveValid,
            epilogue: PlayEpilogue::Default,
            terminal_boundary: false,
        }
    }
}

/// A scripted core serving one session; frames sent through it follow the scripted traffic.
struct Core {
    extra: mpsc::UnboundedSender<Bytes>,
    task: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl Core {
    fn send_batch(&self, packets: &[Packet]) {
        self.extra
            .send(batch(packets))
            .expect("the core is running");
    }

    fn send_raw_packet(&self, id: PacketId, body: &[u8]) {
        self.extra
            .send(raw_packet_batch(id, body))
            .expect("the core is running");
    }

    /// Ends the session and fails on any check the core made of the client's packets.
    async fn finish(self, session: PlaySession) {
        drop(session);
        tokio::time::timeout(CORE_TIMEOUT, self.task)
            .await
            .expect("the core ends with the session")
            .expect("the core's checks of the client pass");
    }
}

/// Joins a scripted core and, as the app does once the world is presented, finishes loading.
async fn join(script: Script) -> Result<(PlaySession, GameData, Core), ProtocolError> {
    let dir = tempfile::tempdir().expect("socket directory");
    let listener = bridge::SessionListener::bind(dir.path())
        .await
        .expect("session endpoint");
    let (extra, frames) = mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let stream = listener.accept().await.expect("the client connects");
        let _ = run_core(ScriptedCore { stream }, script, frames).await;
    });
    let joined = LoginSequence::connect_session(
        dir.path(),
        "RustClient",
        protocol::ClientRandomId::new(-7).unwrap(),
        script.cache_enabled.then(ClientBlobCache::default),
        None,
        &protocol::LoginSettings::default(),
        None,
        None,
        |packs, game_data| (packs, game_data),
    )
    .await;
    let core = Core {
        extra,
        task,
        _dir: dir,
    };
    let (mut session, (_, game_data)) = joined?;
    session.finish_loading().await?;
    Ok((session, game_data, core))
}

/// Joins a script that must spawn.
async fn joined(script: Script) -> (PlaySession, Core) {
    match join(script).await {
        Ok((session, _, core)) => (session, core),
        Err(error) => panic!("scripted join failed: {error}"),
    }
}

struct ScriptedCore {
    stream: bridge::FramedStream,
}

impl ScriptedCore {
    /// Writes one frame; `None` once the client has gone.
    async fn send(&mut self, frame: Bytes) -> Option<()> {
        self.stream.send(frame).await.ok()
    }

    async fn receive(&mut self) -> Option<Bytes> {
        self.stream.next().await?.ok()
    }

    async fn receive_packets(&mut self) -> Option<Vec<Packet>> {
        let frame = self.receive().await?;
        Some(client_packets(&frame))
    }
}

/// Decodes a client Batch frame.
fn client_packets(frame: &[u8]) -> Vec<Packet> {
    assert_eq!(
        frame[0], KIND_BATCH,
        "the client sends only batches after its Connect"
    );
    let mut bedrock = BytesMut::from(&[0xfe][..]);
    bedrock.extend_from_slice(&frame[1..]);
    crate::codec::decode_batch(bedrock.freeze()).expect("client batch decodes")
}

/// A Batch frame holding `packets`.
fn batch(packets: &[Packet]) -> Bytes {
    crate::session_transport::batch_frame(packets).expect("server packets encode")
}

/// A Batch frame holding one entry with `body` written verbatim after the packet ID.
fn raw_packet_batch(id: PacketId, body: &[u8]) -> Bytes {
    let mut packet = Vec::new();
    wire::Writer::new(&mut packet)
        .write_var_u32(id as u32)
        .unwrap();
    packet.extend_from_slice(body);
    let mut frame = vec![KIND_BATCH];
    wire::Writer::new(&mut frame)
        .write_var_u32(packet.len() as u32)
        .unwrap();
    frame.extend_from_slice(&packet);
    frame.into()
}

/// A LevelChunk body whose sub-chunk request limit, -3, is below the schema's -1 floor.
fn negative_limit_level_chunk_body() -> Vec<u8> {
    let mut body = chunk_pos(0, 0).encode_to_vec().unwrap();
    body.extend(DimensionType(0).encode_to_vec().unwrap());
    body.extend(wire::VarUInt(0).encode_to_vec().unwrap());
    body.extend(Some(wire::ZigZag32(-3)).encode_to_vec().unwrap());
    body.extend(false.encode_to_vec().unwrap());
    body.extend(wire::VarUInt(0).encode_to_vec().unwrap());
    body.extend(Bytes::new().encode_to_vec().unwrap());
    body
}

/// The generated packets `packets` carry, for slice patterns over a sent batch.
fn wire_packets(packets: &[Packet]) -> Vec<&packets::Packet> {
    packets.iter().map(|packet| &*packet.data).collect()
}

/// The handoff ending at the `startup` packets, with no packs.
fn handoff_frame(client_cache: bool, startup: &[Packet]) -> Bytes {
    let metadata = serde_json::to_vec(&serde_json::json!({
        "identity": {"display_name": "RustClient", "xuid": "", "uuid": "00000000-0000-4000-8000-000000000001"},
        "client_cache": client_cache,
        "packs_required": false,
        "packs": [],
    }))
    .unwrap();
    let mut frame = BytesMut::new();
    frame.put_u8(KIND_HANDOFF);
    frame.put_u32(metadata.len() as u32);
    frame.extend_from_slice(&metadata);
    frame.extend_from_slice(&batch(startup)[1..]);
    frame.freeze()
}

/// A terminal core message of `kind` with its JSON body.
fn terminal_frame(kind: u8, body: &serde_json::Value) -> Bytes {
    let mut frame = vec![kind];
    frame.extend_from_slice(&serde_json::to_vec(body).unwrap());
    frame.into()
}

async fn run_core(
    mut core: ScriptedCore,
    script: Script,
    mut extra: mpsc::UnboundedReceiver<Bytes>,
) -> Option<()> {
    let connect = core.receive().await?;
    assert_eq!(connect[0], KIND_CONNECT, "the first frame is the Connect");
    let request: serde_json::Value = serde_json::from_slice(&connect[1..]).unwrap();
    assert_eq!(
        request["client_cache"], script.cache_enabled,
        "the client offers its cache only with a resolver installed"
    );

    let mut startup = Vec::new();
    if script.dimension_definition_before_start {
        startup.push(start_game::early_dimension_definition());
    }
    startup.push(start_game(RUNTIME_ID));
    core.send(handoff_frame(script.cache_enabled, &startup))
        .await?;
    if script.conflicting_start {
        core.send(batch(&[start_game(OTHER_RUNTIME_ID)])).await?;
    } else {
        core.send(batch(&[set_time(12_345), set_time(23_456)]))
            .await?;
    }

    let spawn_request = core.receive_packets().await?;
    let requested_radius = protocol::LoginSettings::default().view_distance_chunks;
    assert_eq!(
        spawn_request,
        [
            protocol::request_chunk_radius_packet(
                requested_radius,
                render_api::MAX_VIEW_RADIUS_CHUNKS as u8,
            ),
            protocol::loading_screen_packet(protocol::LoadingScreenPhase::Start, None),
        ],
        "the client requests its radius and starts loading after StartGame"
    );
    let radius = Packet::new(ChunkRadiusUpdated {
        chunk_radius: wire::ZigZag32(i32::from(requested_radius)),
    });
    let spawn = Packet::new(PlayStatus {
        status: PlayStatusType::Playerspawn,
    });
    let prerequisites = match script.order {
        SpawnOrder::RadiusThenSpawn => [item_registry(), radius, spawn],
        SpawnOrder::SpawnThenRadius => [item_registry(), spawn, radius],
    };
    core.send(batch(&prerequisites)).await?;

    let finish = core.receive_packets().await?;
    let finish_packets = wire_packets(&finish);
    assert!(
        matches!(
            finish_packets.as_slice(),
            [
                packets::Packet::ServerboundLoadingScreen(ServerboundLoadingScreen {
                    loading_screen_packet_type: ServerboundLoadingScreenType::Endloadingscreen,
                    ..
                }),
                packets::Packet::SetLocalPlayerAsInitialized(SetLocalPlayerAsInitialized {
                    player_id: ActorRuntimeID(RUNTIME_ID)
                }),
            ]
        ),
        "loading ends with one initialization: {finish:?}"
    );
    for frame in play_traffic(&script) {
        core.send(frame).await?;
    }

    let mut answered = false;
    loop {
        tokio::select! {
            frame = core.receive() => {
                let packets = client_packets(&frame?);
                answer(&mut core, &script, &packets, &mut answered).await?;
            }
            Some(frame) = extra.recv() => core.send(frame).await?,
        }
    }
}

/// The frames the core sends once the client has initialized.
fn play_traffic(script: &Script) -> Vec<Bytes> {
    let wire_failure = matches!(
        script.cache_play_script,
        CachePlayScript::MalformedLevelChunk | CachePlayScript::TrailingLevelChunk
    );
    let mut frames = Vec::new();
    if script.cache_enabled && wire_failure {
        let hash = protocol::client_blob_hash(b"pending-before-wire-failure");
        frames.push(batch(&[
            Packet::new(cached_level_chunk(6, -7, vec![hash], b"pending")),
            set_time(45_678),
        ]));
    }
    match script.cache_play_script {
        CachePlayScript::MalformedLevelChunk => {
            frames.push(raw_packet_batch(PacketId::LevelChunk, &[0xff]));
        }
        CachePlayScript::TrailingLevelChunk => {
            let mut body = LevelChunk::default()
                .encode_to_vec()
                .expect("encode trailing LevelChunk body");
            body.extend_from_slice(&[0xaa]);
            frames.push(raw_packet_batch(PacketId::LevelChunk, &body));
        }
        _ => {}
    }
    let (boundary, terminal) = boundary_epilogue(script);
    if script.cache_enabled && wire_failure {
        frames.push(batch(&[level_chunk(7, -8, 0x4d, 4096)]));
    } else if script.cache_enabled {
        match script.cache_play_script {
            CachePlayScript::ResolveValid => {
                let hash = protocol::client_blob_hash(b"cached-column");
                let mut traffic = vec![
                    level_chunk(8, -10, 0x6b, 1024 * 1024),
                    Packet::new(cached_level_chunk(9, -11, vec![hash], b"tail")),
                    set_time(SCRIPTED_PLAY_END_TIME),
                ];
                traffic.extend(boundary);
                frames.push(batch(&traffic));
                frames.extend(terminal);
            }
            CachePlayScript::TruncatedMissResponse => {
                frames.push(raw_packet_batch(PacketId::ClientCacheMissResponse, &[0x01]))
            }
            CachePlayScript::InvalidMissResponseThenTraffic => {
                let hash = protocol::client_blob_hash(b"semantic-response-wanted");
                frames.push(batch(&[
                    Packet::new(cached_level_chunk(31, -47, vec![hash], b"")),
                    set_time(CACHE_CLASSIFIED_TIME),
                ]));
            }
            CachePlayScript::MalformedLevelChunk | CachePlayScript::TrailingLevelChunk => {
                unreachable!("wire failures are scripted above")
            }
        }
    } else {
        // A well-framed LevelChunk outside its schema bounds (request limit -3) must be skipped,
        // not end the session; the following SetTime still arrives in order. Latency probes
        // ride the same batch: only the from-server probe is answered, scaled, with its flag
        // kept.
        let mut traffic = vec![
            level_chunk(7, -9, 0x5a, 1024 * 1024),
            Packet::new(NetworkStackLatency {
                creation_time: wire::U64LE(777),
                is_from_server: true,
            }),
            Packet::new(NetworkStackLatency {
                creation_time: wire::U64LE(888),
                is_from_server: false,
            }),
            set_time(SCRIPTED_PLAY_END_TIME),
        ];
        traffic.extend(boundary);
        traffic.extend(camera_instruction_epilogue_packets(script.epilogue));
        let mut frame =
            raw_packet_batch(PacketId::LevelChunk, &negative_limit_level_chunk_body()).to_vec();
        frame.extend_from_slice(&batch(&traffic)[1..]);
        frames.push(frame.into());
        if let Some((name, body)) = skipped_epilogue_wire(script.epilogue) {
            let mut frame = raw_packet_batch(name, body).to_vec();
            frame.extend_from_slice(
                &batch(&[set_time(disconnect_reason::EPILOGUE_SENTINEL_TIME)])[1..],
            );
            frames.push(frame.into());
        }
        frames.extend(terminal);
        if let Some((name, body)) = truncated_epilogue_wire(script.epilogue) {
            frames.push(raw_packet_batch(name, body));
        }
    }
    frames
}

/// Reads `count` scripted prelude events, echoing latency probes as the client does.
async fn recv_prelude(session: &mut PlaySession, count: usize) {
    let mut events = 0;
    while events < count {
        match session.recv_world_event(0).await.expect("login prelude") {
            WorldEvent::NetworkStackLatency(creation_time) => session
                .send(protocol::network_stack_latency_reply(creation_time))
                .await
                .unwrap(),
            _ => events += 1,
        }
    }
}

/// Answers one client batch sent during play.
async fn answer(
    core: &mut ScriptedCore,
    script: &Script,
    packets: &[Packet],
    answered: &mut bool,
) -> Option<()> {
    // Vanilla preserves the from-server flag in its echo.
    if let [
        packets::Packet::NetworkStackLatency(NetworkStackLatency {
            creation_time: wire::U64LE(777_000_000),
            is_from_server: true,
        }),
    ] = wire_packets(packets)[..]
    {
        return Some(());
    }
    if script.cache_enabled {
        let (expected, response_payload) = match script.cache_play_script {
            CachePlayScript::ResolveValid => (&b"cached-column"[..], &b"cached-column"[..]),
            CachePlayScript::InvalidMissResponseThenTraffic => (
                &b"semantic-response-wanted"[..],
                &b"semantic-response-poison"[..],
            ),
            CachePlayScript::TruncatedMissResponse => {
                panic!("the truncated response script sends no cached chunk")
            }
            CachePlayScript::MalformedLevelChunk | CachePlayScript::TrailingLevelChunk => {
                return Some(());
            }
        };
        assert!(!*answered, "unexpected client packets: {packets:?}");
        *answered = true;
        let expected_hash = protocol::client_blob_hash(expected);
        assert!(
            matches!(
                wire_packets(packets).as_slice(),
                [packets::Packet::ClientCacheBlobStatus(ClientCacheBlobStatus {
                        missing_ids,
                        found_ids
                    })] if missing_ids == &[wire::U64LE(expected_hash)] && found_ids.is_empty()
            ),
            "the client asks for exactly the missing blob: {packets:?}"
        );
        let mut response = vec![Packet::new(ClientCacheMissResponse {
            missing_blobs: vec![MissingBlobData {
                blob_id: wire::U64LE(expected_hash),
                blob_data: Bytes::copy_from_slice(response_payload),
            }],
        })];
        if script.cache_play_script == CachePlayScript::InvalidMissResponseThenTraffic {
            response.push(set_time(45_678));
        }
        return core.send(batch(&response)).await;
    }
    assert!(!*answered, "unexpected client packets: {packets:?}");
    *answered = true;
    assert!(
        matches!(
            wire_packets(packets).as_slice(),
            [packets::Packet::ClientCacheStatus(ClientCacheStatus {
                is_cache_supported: true
            })]
        ),
        "the test's own play send arrives: {packets:?}"
    );
    // A batch whose only entry declares more bytes than it carries.
    core.send(Bytes::from_static(&[KIND_BATCH, 0x7f])).await
}

fn start_game(runtime_entity_id: u64) -> Packet {
    Packet::new(StartGame {
        runtime_id: ActorRuntimeID(runtime_entity_id),
        ..Default::default()
    })
}

fn set_time(time: i32) -> Packet {
    Packet::new(SetTime {
        time: wire::ZigZag32(time),
    })
}

/// An uncached overworld column at `x, z` whose payload is `len` copies of `fill`.
fn level_chunk(x: i32, z: i32, fill: u8, len: usize) -> Packet {
    Packet::new(LevelChunk {
        chunk_position: chunk_pos(x, z),
        dimension_id: DimensionType(0),
        sub_chunks_count: wire::VarUInt(0),
        serialized_chunk_data: vec![fill; len].into(),
        ..Default::default()
    })
}

/// Builds a cache-enabled LevelChunk for one column referencing `hashes`.
///
/// The blob hashes are written unconditionally and cache participation is stated with
/// `cache_enabled`; gophertunnel `packet/level_chunk.go` expects `SubChunkCount + 1` hashes.
fn cached_level_chunk(x: i32, z: i32, hashes: Vec<u64>, tail: &[u8]) -> LevelChunk {
    let sub_chunks_count = u32::try_from(hashes.len().saturating_sub(1)).expect("fixture count");
    LevelChunk {
        chunk_position: chunk_pos(x, z),
        dimension_id: DimensionType(0),
        sub_chunks_count: wire::VarUInt(sub_chunks_count),
        cache_enabled: true,
        cache_metadata: hashes.into_iter().map(SubChunkMetadata).collect(),
        serialized_chunk_data: Bytes::copy_from_slice(tail),
        ..Default::default()
    }
}

/// A chunk column position in the generated wire shape.
fn chunk_pos(x: i32, z: i32) -> ChunkPos {
    ChunkPos {
        x: wire::ZigZag32(x),
        z: wire::ZigZag32(z),
    }
}

fn item_registry() -> Packet {
    Packet::new(ItemRegistry {
        item_data: vec![ItemData {
            item_name: "minecraft:shield".into(),
            item_id: wire::I16LE(355),
            ..Default::default()
        }],
    })
}

/// Deferred startup packets keep their order, semantic world oddities are skipped and counted,
/// and malformed wire ends the session, whichever order the spawn prerequisites arrive in.
#[tokio::test]
async fn play_keeps_startup_order_and_skips_semantic_world_errors() {
    for order in [SpawnOrder::RadiusThenSpawn, SpawnOrder::SpawnThenRadius] {
        let (mut session, game_data, core) = match join(Script {
            order,
            ..Script::default()
        })
        .await
        {
            Ok(joined) => joined,
            Err(error) => panic!("scripted join failed: {error}"),
        };
        assert_eq!(game_data.world.local_player_runtime_id, RUNTIME_ID);
        assert_eq!(session.decode_error_count(), 0);

        for expected_time in [12_345, 23_456] {
            let deferred =
                tokio::time::timeout(Duration::from_secs(1), session.recv_world_event(0))
                    .await
                    .expect("pre-spawn packet was discarded")
                    .expect("pre-spawn packet must normalize in Play");
            assert_eq!(
                deferred,
                WorldEvent::SetTime(protocol::SetTimeEvent {
                    time: expected_time,
                }),
                "pre-spawn SetTime packets must retain FIFO order"
            );
        }
        let initial_radius =
            tokio::time::timeout(Duration::from_secs(1), session.recv_world_event(0))
                .await
                .expect("initial chunk radius acknowledgement was discarded")
                .expect("initial chunk radius acknowledgement must decode in Play");
        assert!(matches!(initial_radius, WorldEvent::ChunkRadiusUpdated(16)));

        let chunk = tokio::time::timeout(
            Duration::from_secs(1),
            session.recv_world_event_mapped(0, |_| None, |event, payload| Some((event, payload))),
        )
        .await
        .expect("uncached LevelChunk was discarded")
        .expect("uncached LevelChunk must normalize in Play")
        .expect("mapped ingress must select LevelChunk bytes");
        assert_eq!((chunk.0.x, chunk.0.z), (7, -9));
        assert!(chunk.0.payload.is_empty());
        assert_eq!(chunk.1.len(), 1024 * 1024);
        assert!(chunk.1.iter().all(|byte| *byte == 0x5a));

        // Only the from-server probe surfaces; the caller echoes it in stream order.
        assert_eq!(
            session.recv_world_event(0).await.unwrap(),
            WorldEvent::NetworkStackLatency(777)
        );
        session
            .send(protocol::network_stack_latency_reply(777))
            .await
            .unwrap();
        assert_eq!(
            session
                .recv_world_event(0)
                .await
                .expect("post-spawn SetTime must normalize in Play"),
            WorldEvent::SetTime(protocol::SetTimeEvent { time: 34_567 })
        );
        // The preceding malformed chunk was skipped, not fatal.
        assert_eq!(session.world_skip_count(), 1);

        let mut invalid = Packet::new(ClientCacheStatus {
            is_cache_supported: true,
        });
        invalid.header.id = PacketId::PlayStatus;
        let error = session
            .send(invalid)
            .await
            .expect_err("play send must validate the public header");
        assert!(matches!(error, ProtocolError::HeaderIdMismatch { .. }));

        session
            .send(Packet::new(ClientCacheStatus {
                is_cache_supported: true,
            }))
            .await
            .expect("play send");
        let error = session.recv().await.expect_err("malformed batch must fail");
        assert!(matches!(error, ProtocolError::Session(_)));
        assert_eq!(session.decode_error_count(), 1);
        core.finish(session).await;
    }
}

#[tokio::test]
async fn mapped_world_ingress_exposes_latency_before_following_world_events() {
    let (mut session, core) = joined(Script::default()).await;
    for _ in 0..4 {
        session.recv_world_event(0).await.unwrap();
    }
    let probe = session
        .recv_world_event_mapped(0, Some, |_, _| None)
        .await
        .unwrap();
    assert_eq!(probe, Some(WorldEvent::NetworkStackLatency(777)));
    session
        .send(protocol::network_stack_latency_reply(777))
        .await
        .unwrap();
    assert_eq!(
        session.recv_world_event(0).await.unwrap(),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 34_567 })
    );
    core.finish(session).await;
}

#[tokio::test]
async fn the_join_advertises_the_client_cache_only_with_a_resolver() {
    let (session, core) = joined(Script {
        cache_enabled: true,
        ..Script::default()
    })
    .await;
    assert!(session.blob_cache_enabled());
    core.finish(session).await;
}

/// The login prelude every cache-enabled play script starts with.
async fn expect_cached_prelude(session: &mut PlaySession) {
    for expected in [
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 12_345 }),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 23_456 }),
        WorldEvent::ChunkRadiusUpdated(16),
    ] {
        assert_eq!(
            session.recv_world_event(0).await.expect("login prelude"),
            expected
        );
    }
}

/// Supplies the simulation opportunity that the app normally sends after physics.
async fn cache_status_opportunity(session: &mut PlaySession) {
    let mut outbound = session.outbound().unwrap();
    outbound
        .send_batch(Vec::new(), Some(simulation_clock::Moment::default()))
        .await
        .unwrap();
}

#[tokio::test]
async fn play_keeps_normal_output_moving_while_a_cached_chunk_resolves() {
    let (mut session, core) = joined(Script {
        cache_enabled: true,
        ..Script::default()
    })
    .await;
    expect_cached_prelude(&mut session).await;

    let (ordinary, ordinary_payload) = session
        .recv_world_event_mapped(0, |_| None, |event, payload| Some((event, payload)))
        .await
        .expect("resolver-enabled ordinary LevelChunk")
        .expect("ordinary LevelChunk must use byte ingress");
    assert_eq!((ordinary.x, ordinary.z), (8, -10));
    assert!(ordinary.payload.is_empty());
    assert_eq!(ordinary_payload.len(), 1024 * 1024);
    assert!(ordinary_payload.iter().all(|byte| *byte == 0x6b));

    assert_eq!(
        session
            .recv_world_event(0)
            .await
            .expect("independent SetTime"),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 34_567 })
    );

    cache_status_opportunity(&mut session).await;
    let (chunk, payload) = session
        .recv_world_event_mapped(0, |_| None, |event, payload| Some((event, payload)))
        .await
        .expect("resolved cached chunk")
        .expect("cached transaction must resolve through byte ingress");
    assert_eq!((chunk.x, chunk.z), (9, -11));
    assert!(chunk.payload.is_empty());
    assert_eq!(payload, b"cached-columntail".as_slice());

    let stats = session.blob_cache_stats();
    assert_eq!(stats.hashes_classified, 1);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.admitted_blobs, 1);
    assert_eq!(stats.reconstructed_level_chunks, 1);
    assert_eq!(stats.pending_transactions, 0);
    assert_eq!(stats.pending_bytes, 0);
    core.finish(session).await;
}

#[tokio::test]
async fn miss_response_wire_failure_is_fatal_but_semantic_failure_keeps_session_alive() {
    let (mut malformed_session, core) = joined(Script {
        cache_enabled: true,
        cache_play_script: CachePlayScript::TruncatedMissResponse,
        ..Script::default()
    })
    .await;
    expect_cached_prelude(&mut malformed_session).await;
    let error = malformed_session
        .recv_world_event(0)
        .await
        .expect_err("truncated miss-response wire must terminate the session");
    assert!(matches!(error, ProtocolError::Session(_)));
    assert_eq!(malformed_session.decode_error_count(), 1);
    core.finish(malformed_session).await;

    let (mut semantic_session, core) = joined(Script {
        cache_enabled: true,
        cache_play_script: CachePlayScript::InvalidMissResponseThenTraffic,
        ..Script::default()
    })
    .await;
    expect_cached_prelude(&mut semantic_session).await;
    assert_eq!(
        semantic_session.recv_world_event(0).await.unwrap(),
        WorldEvent::SetTime(protocol::SetTimeEvent {
            time: CACHE_CLASSIFIED_TIME
        })
    );
    cache_status_opportunity(&mut semantic_session).await;
    assert_eq!(
        semantic_session
            .recv_world_event(0)
            .await
            .expect("dead transaction emits bounded recovery"),
        WorldEvent::ChunkResync(protocol::ChunkResyncEvent {
            dimension: 0,
            x: 31,
            z: -47,
            requested_sub_chunks: None,
            requested_sub_chunk_ys: None,
        })
    );
    assert_eq!(
        semantic_session
            .recv_world_event(0)
            .await
            .expect("unrelated traffic follows recovery"),
        WorldEvent::SetTime(protocol::SetTimeEvent { time: 45_678 })
    );
    let stats = semantic_session.blob_cache_stats();
    assert_eq!(stats.skipped_miss_responses, 1);
    assert_eq!(stats.rejected_blobs, 1);
    assert_eq!(stats.pending_transactions, 0);
    assert_eq!(stats.abandoned_cached_transactions, 1);
    assert_eq!(stats.recovery_requests, 1);
    assert_eq!(semantic_session.decode_error_count(), 0);
    core.finish(semantic_session).await;
}

#[path = "login_state_cases/start_game.rs"]
mod start_game;
