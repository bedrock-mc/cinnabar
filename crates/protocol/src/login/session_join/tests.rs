use std::path::{Path, PathBuf};

use bytes::{BufMut, Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, ChunkRadiusUpdatedPacket, ItemData, ItemRegistryPacket, PlayStatusPacket,
    StartGamePacket,
};

use super::*;
use crate::BedrockSession;

const RUNTIME_ID: u64 = 7;

/// A core that speaks the session endpoint's frames to one client.
struct FakeCore {
    stream: UnixStream,
}

impl FakeCore {
    async fn accept(listener: &UnixListener) -> Self {
        let (stream, _) = listener.accept().await.expect("client connects");
        Self { stream }
    }

    async fn send(&mut self, frame: Bytes) {
        self.stream.write_u32(frame.len() as u32).await.unwrap();
        self.stream.write_all(&frame).await.expect("core writes");
    }

    async fn receive(&mut self) -> Bytes {
        let length = self.stream.read_u32().await.expect("client frame");
        let mut frame = vec![0; length as usize];
        self.stream
            .read_exact(&mut frame)
            .await
            .expect("readable frame");
        frame.into()
    }

    /// Reads one client Batch frame as packets.
    async fn receive_packets(&mut self) -> Vec<Packet> {
        let frame = self.receive().await;
        assert_eq!(
            frame[0], 2,
            "the client sends only batches after its Connect"
        );
        let mut bedrock = BytesMut::from(&[0xfe][..]);
        bedrock.extend_from_slice(&frame[1..]);
        crate::codec::decode_batch(bedrock.freeze(), &BedrockSession { shield_item_id: 0 })
            .expect("client batch decodes")
    }
}

/// A socket directory removed when the test ends.
struct SocketDir(PathBuf);

impl SocketDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cinnabar-join-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for SocketDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(bridge::session_endpoint_path(&self.0));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn listen(dir: &Path) -> UnixListener {
    UnixListener::bind(bridge::session_endpoint_path(dir)).expect("bind session endpoint")
}

/// One packet's bytes, header included, as a batch carries it.
fn packet_bytes(packet: &Packet) -> Bytes {
    let batch = crate::codec::encode(packet, &BedrockSession { shield_item_id: 0 }).unwrap();
    let CoreMessage::Batch(mut packets) =
        bridge::decode_core_message(bridge::batch_frame_from_bedrock(&batch).unwrap()).unwrap()
    else {
        unreachable!("a Batch frame decodes as a batch")
    };
    packets.remove(0)
}

fn batch_frame(packets: &[Packet]) -> Bytes {
    let mut frame = BytesMut::from(&[2u8][..]);
    for packet in packets {
        let batch = crate::codec::encode(packet, &BedrockSession { shield_item_id: 0 }).unwrap();
        frame.extend_from_slice(&batch[1..]);
    }
    frame.freeze()
}

fn handoff_frame(metadata: serde_json::Value, startup: &[Packet]) -> Bytes {
    let metadata = serde_json::to_vec(&metadata).unwrap();
    let mut frame = BytesMut::new();
    frame.put_u8(3);
    frame.put_u32(metadata.len() as u32);
    frame.extend_from_slice(&metadata);
    for packet in startup {
        let bytes = packet_bytes(packet);
        let mut length = bytes.len() as u32;
        while length >= 0x80 {
            frame.put_u8(length as u8 | 0x80);
            length >>= 7;
        }
        frame.put_u8(length as u8);
        frame.extend_from_slice(&bytes);
    }
    frame.freeze()
}

fn pack_frame(index: u32, bytes: &[u8]) -> Bytes {
    let mut frame = BytesMut::new();
    frame.put_u8(4);
    frame.put_u32(index);
    frame.extend_from_slice(bytes);
    frame.freeze()
}

fn start_game() -> Packet {
    StartGamePacket {
        runtime_id: ActorRuntimeId {
            actor_runtime_id: RUNTIME_ID,
        },
        ..Default::default()
    }
    .into()
}

fn spawn_prerequisites() -> Vec<Packet> {
    vec![
        ItemRegistryPacket {
            item_data: vec![ItemData {
                item_name: "minecraft:shield".into(),
                item_id: 355,
                ..Default::default()
            }],
        }
        .into(),
        ChunkRadiusUpdatedPacket { chunk_radius: 8 }.into(),
        PlayStatusPacket {
            status: jolyne::valentine::PlayStatusPacketStatus::Playerspawn,
        }
        .into(),
    ]
}

fn identity(display_name: &str) -> serde_json::Value {
    serde_json::json!({"display_name": display_name, "xuid": "", "uuid": "00000000-0000-4000-8000-000000000001"})
}

/// The join sends one targetless Connect, then carries the handed-off archives in stack order with
/// their sub-packs, content keys and required bit, and initializes only once presentation is ready.
#[tokio::test]
async fn joins_with_the_handed_off_packs_and_waits_for_presentation() {
    let dir = SocketDir::new("packs");
    let listener = listen(dir.path());
    let core = async {
        let mut core = FakeCore::accept(&listener).await;
        let connect = core.receive().await;
        assert_eq!(connect[0], 1, "the first frame is the Connect");
        let request: serde_json::Value = serde_json::from_slice(&connect[1..]).unwrap();
        assert!(
            request.get("target").is_none(),
            "the join follows the core's selection"
        );
        assert_eq!(request["protocol"], crate::PROTOCOL_VERSION);
        assert_eq!(request["client_data"]["ThirdPartyName"], "Fixture");
        core.send(handoff_frame(
            serde_json::json!({
                "identity": identity("Fixture"), "client_cache": false, "packs_required": true,
                "packs": [
                    {"uuid": "00112233-4455-6677-8899-aabbccddeeff", "version": "1.0.0", "sub_pack": "high", "content_key": "key-a", "size": 5},
                    {"uuid": "11223344-5566-7788-99aa-bbccddeeff00", "version": "2.0.0", "sub_pack": "", "content_key": "", "size": 3},
                ],
            }),
            &[start_game()],
        ))
        .await;
        for (index, bytes) in [(0, &b"ab"[..]), (0, b"cde"), (1, b"xyz")] {
            core.send(pack_frame(index, bytes)).await;
        }
        let spawn_requests = core.receive_packets().await;
        assert!(
            spawn_requests
                .iter()
                .any(|packet| matches!(packet.data, McpePacketData::RequestChunkRadiusPacket(_))),
            "the client runs its own spawn sequence"
        );
        core.send(batch_frame(&spawn_prerequisites())).await;
        core
    };
    let join = LoginSequence::connect_session(
        dir.path(),
        "Fixture",
        Some(ClientBlobCache::default()),
        None,
    );
    let (joined, mut core) = tokio::join!(join, core);
    let (mut session, game_data) = joined.expect("join");
    assert_eq!(game_data.start_game.runtime_id.actor_runtime_id, RUNTIME_ID);
    assert!(
        !session.blob_cache_enabled(),
        "a cache serves only an upstream login that advertised one"
    );

    let handoff = session.take_resource_pack_handoff();
    assert!(handoff.required());
    let archives: Vec<_> = handoff
        .into_archives()
        .into_iter()
        .map(|archive| {
            (
                archive.pack_id.to_string(),
                archive.version,
                archive.sub_pack_name,
                archive.content_key.expose().to_vec(),
                archive.archive,
            )
        })
        .collect();
    assert_eq!(
        archives,
        [
            (
                "00112233-4455-6677-8899-aabbccddeeff".to_owned(),
                "1.0.0".to_owned(),
                "high".to_owned(),
                b"key-a".to_vec(),
                b"abcde".to_vec()
            ),
            (
                "11223344-5566-7788-99aa-bbccddeeff00".to_owned(),
                "2.0.0".to_owned(),
                String::new(),
                Vec::new(),
                b"xyz".to_vec()
            ),
        ]
    );

    // Nothing initializes the player until the owner reports the world presented.
    let early = tokio::time::timeout(std::time::Duration::from_millis(100), core.receive()).await;
    assert!(early.is_err(), "the client initialized before presentation");
    session.finish_loading().await.expect("presentation ready");
    let finish = core.receive_packets().await;
    assert!(finish.iter().any(|packet| matches!(
        &packet.data,
        McpePacketData::SetLocalPlayerAsInitializedPacket(initialized)
            if initialized.player_id.actor_runtime_id == RUNTIME_ID
    )));
}

/// A join the core could not make ends with the server's disconnect screen text.
#[tokio::test]
async fn a_disconnect_before_the_handoff_is_a_server_disconnect() {
    let dir = SocketDir::new("disconnect");
    let listener = listen(dir.path());
    let core = async {
        let mut core = FakeCore::accept(&listener).await;
        core.receive().await;
        let mut frame = BytesMut::from(&[6u8][..]);
        frame.extend_from_slice(
            br#"{"reason":25,"message":"disconnectionScreen.serverFull","filtered_message":"","hide_screen":false}"#,
        );
        core.send(frame.freeze()).await;
        core
    };
    let (joined, _core) = tokio::join!(
        LoginSequence::connect_session(dir.path(), "Fixture", None, None),
        core
    );
    let error = joined.err().expect("the join ends");
    let disconnect = error.server_disconnect().expect("a server disconnect");
    assert_eq!(
        disconnect.message.as_deref(),
        Some("disconnectionScreen.serverFull")
    );
    assert_eq!(disconnect.reason, "Serverfull");
}

/// A transfer before the handoff is followed like a login-time transfer.
#[tokio::test]
async fn a_transfer_before_the_handoff_is_a_server_transfer() {
    let dir = SocketDir::new("transfer");
    let listener = listen(dir.path());
    let core = async {
        let mut core = FakeCore::accept(&listener).await;
        core.receive().await;
        let mut frame = BytesMut::from(&[5u8][..]);
        frame.extend_from_slice(
            br#"{"address":"next.example.test","port":19133,"reload_world":false}"#,
        );
        core.send(frame.freeze()).await;
        core
    };
    let (joined, _core) = tokio::join!(
        LoginSequence::connect_session(dir.path(), "Fixture", None, None),
        core
    );
    let target = joined
        .err()
        .and_then(|error| error.server_transfer())
        .expect("a server transfer");
    assert_eq!(
        (target.host.as_str(), target.port),
        ("next.example.test", 19133)
    );
}

/// The core's disconnect reasons map onto this protocol's reasons; one it does not name keeps its text.
#[test]
fn disconnect_messages_keep_reason_text_and_hidden_screen() {
    let packet = |reason, hide_screen| {
        disconnect_packet(&SessionDisconnect {
            reason,
            message: "kicked".into(),
            filtered_message: "k*****".into(),
            hide_screen,
        })
    };
    let McpePacketData::DisconnectPacket(full) = packet(25, false).data else {
        panic!("a Disconnect packet")
    };
    assert_eq!(full.reason, EnumsConnectionDisconnectFailReason::Serverfull);
    assert_eq!(
        (
            full.messages.message.as_str(),
            full.messages.filtered_message.as_str()
        ),
        ("kicked", "k*****")
    );
    let McpePacketData::DisconnectPacket(unknown) = packet(i32::MAX, true).data else {
        panic!("a Disconnect packet")
    };
    assert_eq!(unknown.reason, EnumsConnectionDisconnectFailReason::Unknown);
    assert!(unknown.hide_disconnection_screen);
    assert_eq!(unknown.messages.message, "kicked");
}
