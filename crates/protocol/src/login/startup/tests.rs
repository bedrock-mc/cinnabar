use bytes::Bytes;
use valentine::bedrock::version::v1_26_51::{LevelChunkPacket, SetTimePacket};

use super::*;
use crate::raw_packet::decode_batch_body;
use crate::session_transport::batch_frame;

fn body(packets: &[Packet]) -> Bytes {
    batch_frame(packets).unwrap().slice(1..)
}

/// A deferred packet no longer shares its batch, and the queue refuses more than its byte bound.
#[test]
fn deferred_packets_are_detached_and_bounded() {
    let batch = body(&[
        Packet::new(SetTimePacket { time: 1 }),
        Packet::new(SetTimePacket { time: 2 }),
    ]);
    let range = batch.as_ptr() as usize..batch.as_ptr() as usize + batch.len();
    let mut deferred = DeferredPackets::default();
    for raw in decode_batch_body(batch.clone(), usize::MAX).unwrap() {
        deferred.push(raw).unwrap();
    }
    for raw in deferred.into_packets() {
        assert!(
            !range.contains(&(raw.inner_frame().as_ptr() as usize)),
            "a deferred packet still holds its whole batch"
        );
    }

    let half = || {
        Packet::new(LevelChunkPacket {
            serialized_chunk_data: vec![0; MAX_DEFERRED_PACKET_BYTES / 2],
            ..Default::default()
        })
    };
    let mut deferred = DeferredPackets::default();
    let chunks = decode_batch_body(body(&[half(), half()]), usize::MAX).unwrap();
    let mut chunks = chunks.into_iter();
    deferred.push(chunks.next().unwrap()).unwrap();
    assert!(matches!(
        deferred.push(chunks.next().unwrap()),
        Err(SessionError(SessionFailure::BatchTooLarge {
            max: MAX_DEFERRED_PACKET_BYTES,
            ..
        }))
    ));
}

#[cfg(unix)]
mod session {
    use std::future::Future;

    use valentine::bedrock::version::v1_26_51::{
        ChunkRadiusUpdatedPacket, ItemData, NetworkChunkPublisherUpdatePacket, PlayStatusPacket,
        TransferPacket,
    };

    use super::*;
    use crate::raw_packet::MAX_RAW_BATCH_PACKETS;
    use crate::session_transport::tests::packet_bytes;
    use crate::session_transport::tests::unix::{Pair, pair, write_frame};

    const RUNTIME_ID: u64 = 42;

    fn start_game(runtime_id: u64) -> Packet {
        Packet::new(StartGamePacket {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id,
            },
            ..Default::default()
        })
    }

    fn registry(name: &str, item_id: i16) -> Packet {
        Packet::new(ItemRegistryPacket {
            item_data: vec![ItemData {
                item_name: name.into(),
                item_id,
                ..Default::default()
            }],
        })
    }

    fn radius(chunk_radius: i32) -> Packet {
        Packet::new(ChunkRadiusUpdatedPacket { chunk_radius })
    }

    fn player_spawn() -> Packet {
        Packet::new(PlayStatusPacket {
            status: EnumsPlayStatus::Playerspawn,
        })
    }

    fn set_time(time: i32) -> Packet {
        Packet::new(SetTimePacket { time })
    }

    /// A transport whose handoff ended at StartGame, with `batches` already sent by the core.
    async fn handed_off(name: &str, batches: &[Vec<Packet>]) -> Pair {
        let mut pair = pair(name, &[packet_bytes(&start_game(RUNTIME_ID))]).await;
        for batch in batches {
            write_frame(&mut pair.peer, &batch_frame(batch).unwrap()).await;
        }
        pair
    }

    async fn next_time(transport: &mut SessionTransport) -> i32 {
        let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
            panic!("a deferred packet arrives before any terminal message")
        };
        let id = raw.id;
        let McpePacketData::SetTimePacket(time) = raw.decode().unwrap().data else {
            panic!("expected SetTime, got {id:?}")
        };
        time.time
    }

    /// A terminal core message of `kind` with its JSON body.
    fn terminal(kind: u8, json: &str) -> Vec<u8> {
        let mut message = vec![kind];
        message.extend_from_slice(json.as_bytes());
        message
    }

    const TRANSFER_JSON: &str =
        r#"{"address":"next.example.test","port":19133,"reload_world":false}"#;

    /// The first registry stays; later empty or custom-only tables are ignored.
    #[tokio::test]
    async fn spawn_keeps_the_first_item_registry() {
        let Pair { transport, .. } = &mut handed_off(
            "first-registry",
            &[vec![
                registry("minecraft:shield", 355),
                Packet::new(ItemRegistryPacket::default()),
                registry("minecraft:shield", 900),
                radius(16),
                player_spawn(),
            ]],
        )
        .await;
        let spawned = await_spawn(transport).await.expect("repeats stay nonfatal");
        assert_eq!(spawned.runtime_id, RUNTIME_ID);
        assert_eq!(spawned.item_registry.item_data.len(), 1);
        assert_eq!(spawned.item_registry.item_data[0].item_id, 355);
    }

    /// Optional packets are deferred to play in order, ahead of a packet the spawning batch
    /// still held, even when they span more packets than one batch may hold.
    #[tokio::test]
    async fn deferred_packets_keep_their_order_across_batches() {
        let half = MAX_RAW_BATCH_PACKETS / 2;
        let mut second: Vec<Packet> = (half..=MAX_RAW_BATCH_PACKETS)
            .map(|time| set_time(time as i32))
            .collect();
        second.extend([
            registry("minecraft:shield", 355),
            radius(16),
            player_spawn(),
            set_time(-1),
        ]);
        let first = (0..half).map(|time| set_time(time as i32)).collect();
        let Pair { transport, .. } = &mut handed_off("fifo", &[first, second]).await;
        await_spawn(transport).await.expect("spawn");
        for expected in 0..=MAX_RAW_BATCH_PACKETS {
            assert_eq!(next_time(transport).await, expected as i32);
        }
        let Inbound::Packet(raw) = transport.recv().await.unwrap() else {
            panic!("the radius reply is deferred")
        };
        assert_eq!(raw.id, McpePacketName::ChunkRadiusUpdatedPacket);
        assert_eq!(next_time(transport).await, -1);
    }

    /// Dragonfly answers with PlayerSpawn alone and streams terrain only after initialization.
    #[tokio::test]
    async fn spawn_reports_whether_terrain_preceded_it() {
        for (name, terrain, expected) in [
            ("no-terrain", None, false),
            (
                "terrain",
                Some(Packet::new(NetworkChunkPublisherUpdatePacket::default())),
                true,
            ),
        ] {
            let mut batch: Vec<Packet> = terrain.into_iter().collect();
            batch.extend([
                registry("minecraft:shield", 355),
                radius(16),
                player_spawn(),
            ]);
            let Pair { transport, .. } = &mut handed_off(name, &[batch]).await;
            let spawned = await_spawn(transport).await.expect("spawn");
            assert_eq!(spawned.terrain_before_spawn, expected);
        }
    }

    /// The spawn sequence requests the radius and starts loading once, right after StartGame.
    #[tokio::test]
    async fn spawn_requests_the_radius_once_after_start_game() {
        use tokio::io::AsyncReadExt;
        let Pair {
            transport, peer, ..
        } = &mut handed_off(
            "radius-request",
            &[vec![
                start_game(RUNTIME_ID),
                registry("minecraft:shield", 355),
                radius(16),
                player_spawn(),
            ]],
        )
        .await;
        await_spawn(transport).await.expect("spawn");
        let length = peer.read_u32().await.unwrap();
        let mut frame = vec![0; length as usize];
        peer.read_exact(&mut frame).await.unwrap();
        assert_eq!(frame, batch_frame(&loading_started()).unwrap());
        let more = tokio::time::timeout(Duration::from_millis(100), peer.read_u8()).await;
        assert!(more.is_err(), "the radius request is sent once");
    }

    #[tokio::test]
    async fn malformed_spawn_prerequisites_end_the_join() {
        for (name, batch, expected) in [
            (
                "conflicting-start",
                vec![start_game(RUNTIME_ID + 1)],
                "conflicting StartGame runtime entity ID",
            ),
            (
                "zero-radius",
                vec![radius(0)],
                "invalid updated chunk radius 0",
            ),
        ] {
            let Pair { transport, .. } = &mut handed_off(name, &[batch]).await;
            let error = match await_spawn(transport).await {
                Ok(_) => panic!("{name} spawned"),
                Err(error) => error,
            };
            assert!(error.to_string().contains(expected), "{name}: {error}");
        }
    }

    /// The bytes deferred while spawning are bounded across batches.
    #[tokio::test]
    async fn spawn_caps_the_deferred_bytes() {
        let half = || {
            Packet::new(LevelChunkPacket {
                serialized_chunk_data: vec![0; MAX_DEFERRED_PACKET_BYTES / 2],
                ..Default::default()
            })
        };
        let Pair {
            transport, peer, ..
        } = &mut handed_off("deferred-cap", &[]).await;
        // The core keeps writing while the client reads; neither batch fits a socket buffer.
        let core = async {
            write_frame(peer, &batch_frame(&[half()]).unwrap()).await;
            let rest = [
                half(),
                registry("minecraft:shield", 355),
                radius(16),
                player_spawn(),
            ];
            write_frame(peer, &batch_frame(&rest).unwrap()).await;
        };
        let (spawned, ()) = tokio::join!(await_spawn(transport), core);
        assert!(matches!(
            spawned,
            Err(ProtocolError::Session(SessionError(
                SessionFailure::BatchTooLarge {
                    max: MAX_DEFERRED_PACKET_BYTES,
                    ..
                }
            )))
        ));
    }

    /// A transfer during spawn ends the join with its target, from the core or in a batch; an
    /// unusable in-batch target is skipped.
    #[tokio::test]
    async fn spawn_ends_with_a_startup_transfer() {
        for from_core in [false, true] {
            let name = format!("startup-transfer-{from_core}");
            let mut pair = handed_off(&name, &[]).await;
            if from_core {
                write_frame(&mut pair.peer, &terminal(5, TRANSFER_JSON)).await;
            } else {
                let transfer = |address: &str| {
                    Packet::new(TransferPacket {
                        server_address: address.into(),
                        server_port: 19133,
                        ..Default::default()
                    })
                };
                let batch = [transfer(""), transfer("next.example.test")];
                write_frame(&mut pair.peer, &batch_frame(&batch).unwrap()).await;
            }
            let target = match await_spawn(&mut pair.transport).await {
                Ok(_) => panic!("a startup transfer spawned"),
                Err(error) => error.server_transfer().expect("a server transfer"),
            };
            assert_eq!(
                (target.host.as_str(), target.port),
                ("next.example.test", 19133)
            );
        }
    }

    /// A disconnect during spawn keeps the server's texts, unless its screen is hidden.
    #[tokio::test]
    async fn spawn_ends_with_a_startup_disconnect() {
        for hide_screen in [false, true] {
            let name = format!("startup-disconnect-{hide_screen}");
            let mut pair = handed_off(&name, &[]).await;
            let json = format!(
                r#"{{"reason":25,"message":"full","filtered_message":"","hide_screen":{hide_screen}}}"#
            );
            write_frame(&mut pair.peer, &terminal(6, &json)).await;
            let disconnect = match await_spawn(&mut pair.transport).await {
                Ok(_) => panic!("a startup disconnect spawned"),
                Err(error) => error.server_disconnect().expect("a server disconnect"),
            };
            assert_eq!(disconnect.reason, "Serverfull");
            let expected = (!hide_screen).then_some("full");
            assert_eq!(disconnect.message.as_deref(), expected);
        }
    }

    /// Without its prerequisites the spawn wait stays pending until its owner drops it.
    #[tokio::test]
    async fn spawn_wait_remains_cancellable() {
        let Pair { transport, .. } = &mut handed_off("cancellable", &[]).await;
        let wait = tokio::time::timeout(Duration::from_millis(100), await_spawn(transport)).await;
        assert!(wait.is_err(), "the wait ended without spawn prerequisites");
    }

    /// A failed startup write reports a transfer or disconnect that arrives instead.
    #[tokio::test]
    async fn startup_write_failure_surfaces_the_terminal_message() {
        let write_error = || ProtocolError::from(SessionFailure::ConnectionClosed);
        let Pair {
            transport, peer, ..
        } = &mut handed_off("write-failure-disconnect", &[]).await;
        write_frame(
            peer,
            &batch_frame(&[Packet::new(
                valentine::bedrock::version::v1_26_51::DisconnectPacket::default(),
            )])
            .unwrap(),
        )
        .await;
        let error =
            startup_write_failure(transport, write_error(), STARTUP_WRITE_FAILURE_DRAIN).await;
        assert!(error.server_disconnect().is_some(), "{error}");

        let Pair {
            transport, peer, ..
        } = &mut handed_off("write-failure-transfer", &[]).await;
        write_frame(peer, &terminal(5, TRANSFER_JSON)).await;
        let error =
            startup_write_failure(transport, write_error(), STARTUP_WRITE_FAILURE_DRAIN).await;
        assert!(error.server_transfer().is_some(), "{error}");
    }

    /// The drain gives up at its deadline and stays cancellable before it.
    #[tokio::test]
    async fn startup_write_failure_drain_obeys_its_deadline() {
        let Pair { transport, .. } = &mut handed_off("write-failure-deadline", &[]).await;
        let error = startup_write_failure(
            transport,
            ProtocolError::from(SessionFailure::ConnectionClosed),
            Duration::ZERO,
        )
        .await;
        assert!(matches!(
            error,
            ProtocolError::Session(SessionError(SessionFailure::ConnectionClosed))
        ));
        let mut drain = std::pin::pin!(startup_write_failure(
            transport,
            ProtocolError::from(SessionFailure::ConnectionClosed),
            STARTUP_WRITE_FAILURE_DRAIN,
        ));
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(drain.as_mut().poll(cx)))
                .await
                .is_pending()
        );
    }
}
