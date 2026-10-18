use super::{arguments, fixtures};
use crate::{capture::read_capture, options::Options, runner::run};
use bytes::Bytes;
use futures::StreamExt;
use protocol::session_wire::{
    ConnectRequest, CoreMessage, HandoffPackReceiver, decode_core_message, encode_connect,
};
use protocol::{ResourcePackArchive, ResourcePackHandoff};
use std::{
    future::pending,
    io::{self, Write},
    time::Duration,
};
use tokio::sync::oneshot;

struct ReadyOutput {
    ready: Option<oneshot::Sender<()>>,
    bytes: Vec<u8>,
}
impl Write for ReadyOutput {
    /// Records output while notifying the client as soon as the endpoint is published.
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(data);
        if String::from_utf8_lossy(&self.bytes).contains("BEDROCK_REPLAY_READY")
            && let Some(ready) = self.ready.take()
        {
            let _ = ready.send(());
        }
        Ok(data.len())
    }
    /// The in-memory output has no pending writes.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Creates an isolated fixture and the same options accepted by the executable.
fn options(root: &std::path::Path) -> Options {
    let mut opts = Options::parse(arguments()).unwrap().unwrap();
    opts.capture = root.join("raw.bin");
    opts.socket_dir = root.join("bridge");
    opts.report = root.join("report.json");
    opts.interval = Duration::ZERO;
    opts.timeout = Duration::from_secs(20);
    opts.burst_packets = 2;
    std::fs::write(&opts.capture, fixtures::capture()).unwrap();
    opts
}

/// Sends exactly the device-free claims the native session client uses for replay.
fn connect() -> Bytes {
    encode_connect(&ConnectRequest { join_profile_id: None, protocol: protocol::PROTOCOL_VERSION, target: None, client_cache: false,
        client_data: serde_json::json!({"GameVersion": protocol::GAME_VERSION, "ThirdPartyName": "ReplayTest"}) }).unwrap()
}

#[tokio::test]
async fn session_round_trip_preserves_packets_packs_reports_and_shutdown() {
    for end in ["client_exit", "fixture_end"] {
        for pack_count in 0..=2 {
            let dir = tempfile::tempdir().unwrap();
            let mut opts = options(dir.path());
            if end == "fixture_end" {
                opts.hold = Duration::from_millis(10);
            }
            let packs = [fixtures::pack(false), fixtures::encrypted_pack()];
            let mut pack_args = Vec::new();
            for (index, pack) in packs.iter().take(pack_count).enumerate() {
                let path = dir.path().join(format!("fixture-{index}.mcpack"));
                std::fs::write(&path, pack).unwrap();
                pack_args.extend([
                    "-resource-pack".to_owned(),
                    path.to_str().unwrap().to_owned(),
                ]);
            }
            if pack_count == 2 {
                pack_args.extend(
                    [
                        "-content-key=",
                        "-content-key",
                        fixtures::ENCRYPTED_PACK_KEY,
                    ]
                    .map(str::to_owned),
                );
            }
            let pack_options = Options::parse(arguments().into_iter().chain(pack_args))
                .unwrap()
                .unwrap();
            opts.packs = pack_options.packs;
            opts.pack_keys = pack_options.pack_keys;
            let expected = read_capture(io::Cursor::new(fixtures::capture())).unwrap();
            let report_path = opts.report.clone();
            let socket_dir = opts.socket_dir.clone();
            let (ready, wait) = oneshot::channel();
            let mut output = ReadyOutput {
                ready: Some(ready),
                bytes: Vec::new(),
            };
            let client = async {
                wait.await.unwrap();
                let (mut reader, frames) = bridge::connect_session(&socket_dir).await.unwrap();
                frames.send(connect()).await.unwrap();
                let CoreMessage::Handoff(handoff) =
                    decode_core_message(reader.next().await.unwrap().unwrap()).unwrap()
                else {
                    panic!("handoff");
                };
                assert_eq!(handoff.packs.len(), pack_count);
                if pack_count > 0 {
                    assert_eq!(handoff.packs[0].content_key.expose(), "");
                }
                if pack_count == 2 {
                    assert_eq!(
                        handoff.packs[1].content_key.expose(),
                        fixtures::ENCRYPTED_PACK_KEY
                    );
                }
                let mut receiver = HandoffPackReceiver::new(&handoff, None).unwrap();
                while !receiver.is_complete() {
                    let CoreMessage::PackData { index, data } =
                        decode_core_message(reader.next().await.unwrap().unwrap()).unwrap()
                    else {
                        panic!("pack data");
                    };
                    receiver.accept(index, &data).unwrap();
                }
                let archives = receiver.into_archives().unwrap();
                assert_eq!(archives, packs[..pack_count]);
                if pack_count == 2 {
                    let stack =
                        resource_pack::validate_handoff(ResourcePackHandoff::from_archives(
                            handoff
                                .packs
                                .iter()
                                .zip(archives)
                                .map(|(pack, archive)| {
                                    ResourcePackArchive::with_content_key(
                                        pack.uuid.parse().unwrap(),
                                        pack.version.clone(),
                                        pack.sub_pack.clone(),
                                        archive,
                                        pack.content_key.expose().as_bytes().to_vec(),
                                    )
                                })
                                .collect(),
                        ));
                    assert!(stack.rejections().is_empty(), "{:?}", stack.rejections());
                    assert_eq!(stack.packs().len(), pack_count);
                    let view = resource_pack::LayeredPackView::new(stack);
                    assert_eq!(
                        view.read("texts/en_US.lang").unwrap().as_ref(),
                        b"replay test\n"
                    );
                }
                let mut packets = handoff.startup;
                while packets.len() < expected.packets.len() {
                    let CoreMessage::Batch(batch) =
                        decode_core_message(reader.next().await.unwrap().unwrap()).unwrap()
                    else {
                        panic!("batch");
                    };
                    packets.extend(batch);
                }
                assert_eq!(
                    packets,
                    expected
                        .packets
                        .iter()
                        .map(|packet| packet.wire.clone())
                        .collect::<Vec<_>>()
                );
                if end == "fixture_end" {
                    assert!(reader.next().await.is_none());
                }
                drop(frames);
                drop(reader);
            };
            let (result, ()) = tokio::join!(run(opts, &mut output, pending()), client);
            result.unwrap();
            assert!(!bridge::session_endpoint_path(&socket_dir).exists());
            let report: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&report_path).unwrap()).unwrap();
            assert_eq!(report["end_reason"], end);
            assert_eq!(report["complete"], true);
            assert!(report.get("error").is_none());
            assert_eq!(report["replay"]["sha256"], expected.summary.replay_sha256);
            assert_eq!(report["replay"]["bursts"].as_array().unwrap().len(), 4);
            if pack_count > 0 {
                use sha2::{Digest, Sha256};
                for (index, pack) in packs.iter().take(pack_count).enumerate() {
                    assert_eq!(
                        report["packs"][index]["SHA256"],
                        format!("{:x}", Sha256::digest(pack))
                    );
                }
            } else {
                assert!(report["packs"].is_null());
            }
            assert!(
                String::from_utf8(output.bytes)
                    .unwrap()
                    .contains("BEDROCK_REPLAY_DELIVERED")
            );
        }
    }
}

#[tokio::test]
async fn native_login_joins_the_replay_session() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path());
    let socket_dir = opts.socket_dir.clone();
    let (ready, wait) = oneshot::channel();
    let mut output = ReadyOutput {
        ready: Some(ready),
        bytes: Vec::new(),
    };
    let client = async {
        wait.await.unwrap();
        let (mut session, _) = protocol::LoginSequence::connect_session(
            &socket_dir,
            "Fixture",
            protocol::ClientRandomId::new(-7).unwrap(),
            None,
            None,
            &Default::default(),
            None,
            |_, _| (),
        )
        .await
        .unwrap();
        session.finish_loading().await.unwrap();
        for _ in 0..2 {
            session.recv().await.unwrap();
        }
        drop(session);
    };
    let (result, ()) = tokio::join!(run(opts, &mut output, pending()), client);
    result.unwrap();
}

#[tokio::test(start_paused = true)]
async fn deadline_reports_incomplete_and_cancellation_wins_ties() {
    for cancelled in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut opts = options(dir.path());
        opts.timeout = Duration::from_secs(1);
        let report_path = opts.report.clone();
        let error = if cancelled {
            run(opts, &mut Vec::new(), async {}).await.unwrap_err()
        } else {
            run(opts, &mut Vec::new(), pending()).await.unwrap_err()
        };
        assert!(error.to_string().contains(if cancelled {
            "context canceled"
        } else {
            "context deadline exceeded"
        }));
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(report_path).unwrap()).unwrap();
        assert_eq!(report["complete"], false);
        assert_eq!(report["replay"]["packets"], 0);
        assert!(report["replay"]["bursts"].is_null());
        assert!(report.get("error").is_some());
    }
}

#[tokio::test]
async fn reused_report_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let opts = options(dir.path());
    let path = opts.report.clone();
    std::fs::write(&path, b"previous measurement").unwrap();
    assert!(run(opts, &mut Vec::new(), pending()).await.is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"previous measurement");
}

#[test]
fn nested_pack_keeps_input_hash_and_delivers_the_inner_archive() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested.zip");
    let bytes = fixtures::pack(true);
    std::fs::write(&path, &bytes).unwrap();
    let pack = crate::packs::read_pack(&path, "").unwrap();
    assert_eq!(pack.archive.as_ref(), fixtures::pack(false));
    assert_eq!(pack.metadata.version, "1.0.0");
    assert_eq!(pack.summary.sha256, format!("{:x}", Sha256::digest(bytes)));
}
