use super::*;

/// Commits real request-mode columns before exercising the app's transport handoff.
fn requested_columns(xs: &[i32]) -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    for (index, &x) in xs.iter().enumerate() {
        stream
            .submit(
                index as u64 + 1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: overworld_biome_payload(),
                }),
            )
            .unwrap();
    }
    complete_world_stream_decodes(&mut stream);
    stream
}

#[test]
fn groups_admitted_columns_in_one_request() {
    let mut stream = requested_columns(&[0, 1]);
    let mut packets = Vec::new();
    let mut receipts = Vec::new();
    let sent = flush_sub_chunk_requests(&mut stream, 8, [0; 3], |receipt, packet| {
        receipts.push(receipt);
        packets.push(packet);
        Ok(())
    })
    .unwrap();

    assert_eq!(sent, 1, "admitted columns share one request envelope");
    assert_eq!(packets.len(), 1);
    assert_eq!(stream.pending_request_count(), 0);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
    assert_eq!(
        receipts[0]
            .members
            .iter()
            .map(|member| member.position)
            .collect::<Vec<_>>(),
        [[0, -4, 0], [1, -4, 0]]
    );
    assert_eq!(
        stream
            .publication_snapshot(ChunkKey::new(0, 0, 0))
            .stages
            .requests_transport_pending,
        1
    );
    stream.acknowledge_sub_chunk_request_sent(&receipts[0], Instant::now());
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 2);
}

#[test]
fn full_outbound_queue_retries_the_same_request_then_preserves_fifo_order() {
    let mut stream = requested_columns(&[0, 1, 2]);

    let mut receipts = Vec::new();
    assert_eq!(
        flush_sub_chunk_requests(&mut stream, 1, [0; 3], |receipt, _| {
            receipts.push(receipt);
            Ok(())
        })
        .unwrap(),
        1
    );
    let mut attempts = Vec::new();
    assert_eq!(
        flush_sub_chunk_requests(&mut stream, 2, [0; 3], |receipt, packet| {
            attempts.push(
                receipt
                    .members
                    .iter()
                    .map(|member| member.position)
                    .collect::<Vec<_>>(),
            );
            Err(client_session::PacketSendError::Full(packet))
        })
        .unwrap(),
        0
    );
    assert_eq!(stream.pending_request_count(), 2);
    stream.acknowledge_sub_chunk_request_sent(&receipts[0], Instant::now());
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);
    assert_eq!(
        flush_sub_chunk_requests(&mut stream, 2, [0; 3], |receipt, _| {
            attempts.push(
                receipt
                    .members
                    .iter()
                    .map(|member| member.position)
                    .collect::<Vec<_>>(),
            );
            receipts.push(receipt);
            Ok(())
        })
        .unwrap(),
        1
    );
    assert_eq!(attempts, vec![vec![[1, -4, 0], [2, -4, 0]]; 2]);
    assert_eq!(stream.pending_request_count(), 0);
    stream.acknowledge_sub_chunk_request_sent(&receipts[1], Instant::now());
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 3);
}

#[test]
fn command_admission_leaves_deadline_unarmed_until_transport_success_acknowledgement() {
    let mut stream = requested_columns(&[0]);
    let mut receipt = None;

    assert_eq!(
        flush_sub_chunk_requests(&mut stream, 1, [0; 3], |sent, _| {
            receipt = Some(sent);
            Ok(())
        })
        .unwrap(),
        1
    );
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
    let acknowledged_at = Instant::now() + Duration::from_secs(100);

    stream.acknowledge_sub_chunk_request_sent(&receipt.unwrap(), acknowledged_at);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);

    let mut failed = requested_columns(&[0]);
    assert_eq!(
        flush_sub_chunk_requests(&mut failed, 1, [0; 3], |_, packet| {
            Err(client_session::PacketSendError::Full(packet))
        })
        .unwrap(),
        0
    );
    assert_eq!(failed.stats().awaiting_sub_chunk_responses, 0);
    assert_eq!(failed.stats().sub_chunk_timeouts, 0);
}

#[test]
fn network_session_fatal_is_retained_when_command_sender_closes_in_same_frame() {
    let mut stream = requested_columns(&[0, 1]);
    let original = "network session failed: Protocol error: original fatal";
    let mut fatal_error = None;
    record_fatal_error(&mut fatal_error, original.to_owned());

    let closed = flush_sub_chunk_requests(&mut stream, 2, [0; 3], |_, packet| {
        Err(client_session::PacketSendError::Closed(packet))
    })
    .unwrap_err();
    record_fatal_error(&mut fatal_error, closed);

    assert_eq!(fatal_error.as_deref(), Some(original));
    assert_eq!(stream.pending_request_count(), 2);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
}

#[test]
fn client_world_publication_contract_crosses_the_app_boundary() {
    let mut stream = requested_columns(&[0]);

    let batch = stream
        .prepare_sub_chunk_request([0; 3], 1)
        .expect("sub-chunk request");
    stream.record_sub_chunk_request_transport_pending(&batch);
    stream.acknowledge_sub_chunk_request_sent(&batch.receipt, Instant::now());
    let [x, y, z] = batch.receipt.members[0].position;
    let key = SubChunkKey::new(batch.receipt.dimension, x, y, z);
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    diagnostics: None,
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    complete_world_stream_decodes(&mut stream);

    let acknowledgement = super::super::core::wait_for_mesh_publication(|| {
        stream.poll([0.0; 3], 1);
        let mut target = None;
        while let Some(change) = stream.pop_mesh_change() {
            let (changed, generation, dirty_since) = match change {
                WorldMeshChange::Upsert {
                    key,
                    generation,
                    dirty_since,
                    ..
                }
                | WorldMeshChange::Remove {
                    key,
                    generation,
                    dirty_since,
                    ..
                } => (key, generation, dirty_since),
            };
            stream.acknowledge_mesh_upload(changed, generation, dirty_since, Instant::now());
            if changed == key {
                target = Some((generation, dirty_since));
            }
        }
        std::thread::yield_now();
        target
    })
    .unwrap_or_else(|| panic!("public mesh publication timed out: {:?}", stream.stats()));
    assert_ne!(acknowledgement.0, 0);
    assert!(stream.is_mesh_clean(key));
}

#[test]
fn zero_world_admission_still_drains_control_ack_and_leaves_world_fifo_untouched() {
    let (control_sender, mut control_receiver) = tokio::sync::mpsc::channel(1);
    let sent_at = Instant::now();
    control_sender
        .try_send(NetworkControlEvent::SubChunkRequestSent {
            receipt: render_api::sub_chunk_requests::SubChunkRequestReceipt {
                owner: (7, 0),
                id: 1,
                dimension: 0,
                members: vec![render_api::sub_chunk_requests::SubChunkRequestMember {
                    position: [3, -4, -2],
                    demand: 1,
                }]
                .into(),
            },
            sent_at,
        })
        .unwrap();
    let (world_sender, mut world_receiver) = tokio::sync::mpsc::channel(1);
    world_sender
        .try_send(SequencedWorldEvent {
            session_generation: SessionGeneration::new(7),
            sequence: FifoSequence::new(1),
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
        .unwrap();

    let controls = drain_network_controls(&mut control_receiver, OUTBOUND_SEND_BUDGET_PER_FRAME);
    let world = drain_network_ingress(&mut world_receiver, 0);

    assert!(matches!(
        controls.as_slice(),
        [NetworkControlEvent::SubChunkRequestSent {
            receipt, sent_at: observed,
        }] if receipt.members[0].position == [3, -4, -2] && *observed == sent_at
    ));
    assert!(world.is_empty());
    assert!(matches!(
            world_receiver.try_recv(),
            Ok(SequencedWorldEvent {
                session_generation,
                sequence,
                event: WorldEvent::ChunkRadiusUpdated(16),
            }) if session_generation.get() == 7 && sequence.get() == 1
    ));
}
