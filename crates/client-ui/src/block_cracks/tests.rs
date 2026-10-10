use chunk_pipeline::WorldStream;
use client_world::CommittedUiEvent;
use protocol::{BlockCrackAction, WorldBootstrap, WorldEvent};

use {super::*, chunk_pipeline::BlockCrackStatus};

fn event(position: [i32; 3], action: BlockCrackAction) -> BlockCrackEvent {
    BlockCrackEvent { position, action }
}

fn start(value: i32) -> BlockCrackAction {
    BlockCrackAction::Start {
        progress_per_tick: value,
    }
}

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn block_crack_production_stream_batches_consume_beyond_former_history_limit() {
    let mut stream = stream();
    let mut ui = UiRuntime::new(9);
    ui.note_stream_dimension(0);
    for batch in 0..40_u64 {
        for offset in 0..32_u64 {
            stream
                .submit(
                    batch * 32 + offset + 1,
                    WorldEvent::BlockCrack(event([0; 3], BlockCrackAction::Stop)),
                )
                .unwrap();
        }
        for committed in stream.take_committed_ui() {
            let CommittedUiEvent::BlockCrack {
                sequence,
                dimension,
                event,
            } = committed
            else {
                panic!("expected committed crack");
            };
            consume_committed_block_crack(&mut ui, 9, sequence, dimension, event).unwrap();
        }
        reconcile_world_block_cracks(&mut ui, &stream);
        assert_eq!(ui.block_cracks_status().active, 0);
    }
    assert_eq!(ui.block_cracks_status().consumed, 1_280);
    assert!(stream.take_committed_ui().is_empty());
}

fn committed_column(stream: &mut WorldStream, sequence: u64, runtime_id: u32, biome: u8) {
    let mut payload = vec![9, 1, (-4_i8) as u8, 1];
    let mut encoded = runtime_id << 1;
    loop {
        let byte = (encoded & 0x7f) as u8;
        encoded >>= 7;
        payload.push(byte | if encoded == 0 { 0 } else { 0x80 });
        if encoded == 0 {
            break;
        }
    }
    payload.extend([1, biome << 1]);
    payload.extend(std::iter::repeat_n(0xff, 23));
    payload.push(0);
    stream
        .submit(
            sequence,
            WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::Inline { count: 1 },
                payload,
            }),
        )
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        stream.poll([0.0; 3], 0);
        assert!(stream.take_fatal_error().is_none());
        if stream
            .collision_store()
            .sub_chunk(world::SubChunkKey::new(0, 0, -4, 0))
            .is_some_and(|chunk| chunk.runtime_id(0, 0, 0, 0) == Some(runtime_id))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "column commit timed out"
        );
        std::thread::yield_now();
    }
}

fn drain_cracks(ui: &mut UiRuntime, stream: &mut WorldStream) {
    ui.note_stream_dimension(stream.current_dimension());
    for committed in stream.take_committed_ui() {
        if let CommittedUiEvent::BlockCrack {
            sequence,
            dimension,
            event,
        } = committed
        {
            consume_committed_block_crack(ui, 9, sequence, dimension, event).unwrap();
        }
    }
    reconcile_world_block_cracks(ui, stream);
}

#[test]
fn block_crack_production_start_does_not_bind_to_later_replacement() {
    let mut stream = stream();
    let mut ui = UiRuntime::new(9);
    committed_column(&mut stream, 1, 0, 1);
    stream
        .submit(2, WorldEvent::BlockCrack(event([0, -64, 0], start(7))))
        .unwrap();
    committed_column(&mut stream, 3, 1, 1);
    committed_column(&mut stream, 4, 0, 1);
    drain_cracks(&mut ui, &mut stream);
    assert_eq!(ui.block_cracks_status().active, 0);
}

#[test]
fn block_crack_production_unload_reload_identical_target_retires_start() {
    let mut stream = stream();
    let mut ui = UiRuntime::new(9);
    committed_column(&mut stream, 1, 0, 1);
    stream
        .submit(2, WorldEvent::BlockCrack(event([0, -64, 0], start(7))))
        .unwrap();
    // A changed biome column request commits through the production eviction path.
    let mut payload = vec![1, 4];
    payload.extend(std::iter::repeat_n(0xff, 23));
    payload.push(0);
    stream
        .submit(
            3,
            WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: protocol::LevelChunkMode::LimitlessRequests,
                payload,
            }),
        )
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while stream
        .collision_store()
        .sub_chunk(world::SubChunkKey::new(0, 0, -4, 0))
        .is_some()
    {
        stream.poll([0.0; 3], 0);
        assert!(stream.take_fatal_error().is_none());
        assert!(
            std::time::Instant::now() < deadline,
            "eviction commit timed out"
        );
        std::thread::yield_now();
    }
    committed_column(&mut stream, 4, 0, 2);
    drain_cracks(&mut ui, &mut stream);
    assert_eq!(ui.block_cracks_status().active, 0);
}

#[test]
fn block_crack_production_dimension_round_trip_does_not_resurrect_start() {
    let mut stream = stream();
    let mut ui = UiRuntime::new(9);
    committed_column(&mut stream, 1, 0, 1);
    stream
        .submit(2, WorldEvent::BlockCrack(event([0, -64, 0], start(7))))
        .unwrap();
    for (sequence, dimension) in [(3, 1), (4, 0)] {
        stream
            .submit(
                sequence,
                WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                    dimension,
                    position: [0.0; 3],
                    ..Default::default()
                }),
            )
            .unwrap();
    }
    committed_column(&mut stream, 5, 0, 1);
    drain_cracks(&mut ui, &mut stream);
    assert_eq!(ui.block_cracks_status().active, 0);
}

#[test]
fn block_crack_projection_disconnect_and_session_reset_preserve_identity_rules() {
    let mut player_runtime = player_state::PlayerState::new(9);

    let mut stream = stream();
    let mut ui = UiRuntime::new(9);
    committed_column(&mut stream, 1, 0, 1);
    stream
        .submit(2, WorldEvent::BlockCrack(event([0, -64, 0], start(7))))
        .unwrap();
    drain_cracks(&mut ui, &mut stream);
    assert_eq!(ui.block_cracks_status().active, 1);
    assert_eq!(ui.block_cracks_status().server_value_sum, 7);
    ui.clear_disconnected_block_cracks();
    assert_eq!(ui.block_cracks_status().active, 0);
    assert!(matches!(
        consume_committed_block_crack(&mut ui, 9, 2, 0, event([0, -64, 0], start(7))),
        Err(UiRuntimeError::StaleBlockCrackSequence { .. })
    ));
    player_runtime.begin_session(10);
    ui.begin_session(10);
    assert_eq!(ui.block_cracks_status(), BlockCrackStatus::default());
    assert!(matches!(
        consume_committed_block_crack(&mut ui, 9, 3, 0, event([0, -64, 0], start(7))),
        Err(UiRuntimeError::WrongSession { .. })
    ));
    consume_committed_block_crack(&mut ui, 10, 1, 0, event([0, -64, 0], start(7))).unwrap();
    assert_eq!(ui.block_cracks_status().active, 0);
}
