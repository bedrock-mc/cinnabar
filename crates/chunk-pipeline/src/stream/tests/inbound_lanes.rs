use super::*;

fn remote_move(runtime_id: u64) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id,
        position: [Some(1.0); 3],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: None,
        yaw: None,
        head_yaw: None,
        on_ground: None,
        teleported: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    }))
}

fn block_update(chunk_x: i32, network_id: u32) -> WorldEvent {
    WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: 0,
        position: [chunk_x * 16, -64, 0],
        layer: 0,
        network_id,
    }])
}

fn take_decode_job(
    stream: &mut WorldStream,
    matches: impl Fn(&super::DecodeJob) -> bool,
) -> super::QueuedDecodeJob {
    let index = stream
        .pending_decode
        .iter()
        .position(|queued| matches(&queued.job))
        .expect("the decode job is queued");
    stream.pending_decode.remove(index).unwrap()
}

#[test]
fn light_events_commit_with_zero_polls_while_heavy_admission_is_full() {
    let mut stream = block_entity_visual_stream();
    let heavy = MAX_ADMITTED_HEAVY_EVENTS as u64;
    for sequence in 1..=heavy {
        stream
            .submit(sequence, inline_air_event(sequence as i32))
            .unwrap();
    }
    assert_eq!(stream.order.heavy_capacity(), 0);
    assert!(stream.light_admission_capacity() > 0);
    stream.submit(heavy + 1, inline_air_event(0)).unwrap();
    assert_eq!(stream.order.deferred_count(), 1);
    assert!(stream.remaining_admission_capacity() > 0);
    stream.submit(heavy + 2, remote_move(9)).unwrap();
    stream
        .submit(heavy + 3, WorldEvent::NetworkStackLatency(5))
        .unwrap();
    assert!(stream.order.is_finished(heavy + 2));
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [CommittedControlEvent::NetworkStackLatency {
            creation_time: 5,
            ..
        }]
    ));
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.committed_sequence(), heavy + 3);
}

#[test]
fn actor_move_after_a_block_decode_on_another_column_commits_without_waiting() {
    let mut stream = block_entity_visual_stream();
    stream.submit(1, inline_air_event(0)).unwrap();
    stream.submit(2, inline_air_event(1)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.submit(3, worker_block_batch(0, 1)).unwrap();
    assert_eq!(stream.order.blocking_block_updates(), Some(3));
    stream.submit(4, remote_move(9)).unwrap();
    assert!(stream.order.is_finished(4));
    stream.submit(5, worker_block_batch(1, 1)).unwrap();
    assert!(
        !stream.order.is_finished(5),
        "one block decode is in flight at a time"
    );
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.committed_sequence(), 5);
}

#[test]
fn actor_move_after_a_level_chunk_commits_without_waiting() {
    let mut stream = block_entity_visual_stream();
    stream.submit(1, inline_air_event(0)).unwrap();
    stream.submit(2, remote_move(9)).unwrap();
    assert!(stream.order.is_finished(2));
    assert_eq!(stream.committed_sequence(), 0);
}

#[test]
fn block_update_on_a_pending_column_waits_only_for_that_column() {
    let mut stream = block_entity_visual_stream();
    stream.submit(1, inline_air_event(1)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.submit(2, inline_air_event(0)).unwrap();
    stream.submit(3, worker_block_batch(0, 1)).unwrap();
    stream.submit(4, worker_block_batch(1, 1)).unwrap();
    assert_eq!(stream.order.blocking_block_updates(), Some(4));
    let job = take_decode_job(&mut stream, |job| {
        matches!(job, super::DecodeJob::BlockUpdates { .. })
    });
    complete_decode_job(&mut stream, job);
    assert!(stream.order.is_finished(4));
    assert!(!stream.order.is_finished(3));
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.committed_sequence(), 4);
}

#[test]
fn move_commits_in_the_frame_whose_budget_sub_chunk_commits_spent() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    stream.enqueue_request(key.chunk(), key.y, 3, None);
    stream
        .order
        .admit(2, Footprint::terrain([key.chunk()], true), 0)
        .unwrap();
    stream
        .order
        .insert_ready(
            2,
            PreparedWorldEvent::SubChunks {
                dimension: key.dimension,
                entries: (0..3)
                    .map(|offset| PreparedSubChunk {
                        diagnostics: None,
                        position: [key.x, key.y + offset, key.z],
                        result: PreparedSubChunkResult::AllAir,
                    })
                    .collect(),
                duration: Duration::ZERO,
            },
        )
        .unwrap();
    stream.begin_frame_work();
    stream.poll_deadline = Some(Instant::now());
    stream.polling = true;
    stream.apply_ready();
    stream.polling = false;
    assert!(stream.order.pending_batch_sequence().is_some());
    stream.submit(3, remote_move(9)).unwrap();
    assert!(stream.order.is_finished(3));
}

/// Final terrain and control history after lane commits equal strict wire-order application.
#[test]
fn randomized_interleavings_match_strict_fifo_world_state() {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    for _ in 0..60 {
        let count = 4 + next(16);
        let events = (0..count)
            .map(|_| match next(5) {
                0 => inline_block_entity_event(next(3) as i32, 1 + next(3) as u32, Vec::new()),
                1 => block_update(next(3) as i32, next(4) as u32),
                2 => remote_move(9),
                3 => WorldEvent::NetworkStackLatency(next(1000)),
                _ => WorldEvent::SetTime(SetTimeEvent {
                    time: next(1000) as i32,
                }),
            })
            .collect::<Vec<_>>();

        let mut strict = block_entity_visual_stream();
        for (index, event) in events.iter().enumerate() {
            strict.submit(index as u64 + 1, event.clone()).unwrap();
            complete_pending_decode_jobs(&mut strict);
        }

        let mut lanes = block_entity_visual_stream();
        for (index, event) in events.into_iter().enumerate() {
            lanes.submit(index as u64 + 1, event).unwrap();
        }
        while !lanes.pending_decode.is_empty() {
            let index = next(lanes.pending_decode.len() as u64) as usize;
            let job = lanes.pending_decode.remove(index).unwrap();
            complete_decode_job(&mut lanes, job);
        }
        complete_pending_decode_jobs(&mut lanes);

        assert_eq!(lanes.committed_sequence(), count);
        assert_eq!(
            lanes.take_committed_controls(),
            strict.take_committed_controls()
        );
        for x in 0..3 {
            let key = SubChunkKey::new(0, x, -4, 0);
            let cell = |stream: &WorldStream| {
                stream
                    .authority
                    .terrain()
                    .sub_chunk(key)
                    .map(|sub_chunk| sub_chunk.runtime_id(0, 0, 0, 0))
            };
            assert_eq!(cell(&lanes), cell(&strict), "column {x}");
        }
    }
}

/// A crack admitted after a delayed dimension change belongs to the new dimension's column,
/// so it waits for that column's chunk instead of overtaking it and being dropped.
#[test]
fn crack_after_a_delayed_dimension_change_waits_for_its_new_column() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 2,
            block_network_ids_are_hashes: false,
        },
        Arc::new(non_default_air_runtime_assets()),
        [0.0; 3],
        None,
    );
    let solid_column = |dimension: i32, y: i8| {
        let mut payload = vec![9, 1, y as u8, 1, 0];
        payload.extend(biome_payload(dimension, 1));
        WorldEvent::LevelChunk(LevelChunkEvent {
            dimension,
            x: 0,
            z: 0,
            mode: LevelChunkMode::Inline { count: 1 },
            payload,
        })
    };
    stream.submit(1, solid_column(0, -4)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                ..Default::default()
            }),
        )
        .unwrap();
    stream.submit(3, solid_column(1, 0)).unwrap();
    stream
        .submit(
            4,
            WorldEvent::BlockCrack(BlockCrackEvent {
                position: [0, 0, 0],
                action: BlockCrackAction::Start {
                    progress_per_tick: 7,
                },
            }),
        )
        .unwrap();
    let old_dimension = take_decode_job(
        &mut stream,
        |job| matches!(job, super::DecodeJob::InlineLevelChunk { event, .. } if event.dimension == 0),
    );
    complete_decode_job(&mut stream, old_dimension);
    stream.apply_ready();
    assert!(
        stream.order.is_finished(2),
        "the dimension change committed"
    );
    assert!(
        !stream.order.is_finished(4),
        "the crack waits for its column"
    );
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.current_dimension(), 1);
    assert_eq!(stream.block_crack_snapshot().status.active, 1);
}
