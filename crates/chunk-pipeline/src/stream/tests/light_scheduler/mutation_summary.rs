use super::*;

/// Builds equal-light updates over 64 independent packed sub-chunks.
fn mutation_fixture() -> (WorldStream, Vec<super::super::super::BlockMutationBatch>) {
    let mut stream = lit_stream(0);
    stream.set_custom_block_ids(4..5);
    let batches = (0..64)
        .map(|x| {
            let key = SubChunkKey::new(0, x, 0, 0);
            stream
                .authority
                .commit_sub_chunk(key, uniform_sub_chunk(2))
                .unwrap();
            stream.resident.insert(key);
            super::super::super::BlockMutationBatch {
                key,
                previous: stream.authority.terrain().sub_chunk(key),
                updates: vec![BlockUpdate {
                    layer: 0,
                    x: 15,
                    y: 15,
                    z: 15,
                    runtime_id: 4,
                }],
            }
        })
        .collect();
    (stream, batches)
}

/// Measures worker preparation separately from frame-thread publication.
#[test]
#[ignore = "benchmark"]
fn mutation_light_summary_cost() {
    let mut worker_us = Vec::new();
    let mut commit_us = Vec::new();
    for _ in 0..32 {
        let (mut stream, batches) = mutation_fixture();
        let start = Instant::now();
        let prepared =
            client_world::ingestion::prepare_block_mutations(batches, &stream.decode_ids(0))
                .unwrap();
        worker_us.push(start.elapsed().as_secs_f64() * 1e6);
        let start = Instant::now();
        assert!(stream.commit_block_mutations_with_relight(prepared.mutations, &prepared.relight));
        commit_us.push(start.elapsed().as_secs_f64() * 1e6);
        assert!(stream.lighting.jobs.pending.is_empty());
        for x in 0..64 {
            assert_eq!(
                stream
                    .authority
                    .terrain()
                    .sub_chunk(SubChunkKey::new(0, x, 0, 0))
                    .unwrap()
                    .runtime_id(0, 15, 15, 15),
                Some(4)
            );
        }
    }
    worker_us.sort_by(f64::total_cmp);
    commit_us.sort_by(f64::total_cmp);
    eprintln!(
        "MUTATION_SUMMARY count=64 worker_p50_us={:.3} worker_p95_us={:.3} commit_p50_us={:.3} commit_p95_us={:.3}",
        worker_us[16], worker_us[30], commit_us[16], commit_us[30]
    );
}

/// Worker summaries preserve light invalidation, packed blocks, and dirty mesh targets.
#[test]
fn worker_light_summary_matches_direct_prediction_commit() {
    for (previous_id, next_id, layer, relights) in [
        (None, 0, 0, false),
        (None, 1, 0, true),
        (Some(0), 0, 0, true), // The redundant air storage is removed.
        (Some(0), 3, 0, true),
        (Some(3), 0, 0, true),
        (Some(2), 4, 0, false),
        (Some(2), 2, 0, false),
        (Some(2), 1, 0, true),
        (Some(3), 1, 1, true),
        (Some(1), 2, 1, true),
    ] {
        let key = SubChunkKey::new(0, 0, 0, 0);
        let mut direct = lit_stream(0);
        let mut worker = lit_stream(0);
        for stream in [&mut direct, &mut worker] {
            stream.set_custom_block_ids(4..5);
            if let Some(id) = previous_id {
                stream
                    .authority
                    .commit_sub_chunk(key, uniform_sub_chunk(id))
                    .unwrap();
                stream.resident.insert(key);
            }
        }
        let updates = vec![BlockUpdate::new(15, 15, 15, layer, next_id)];
        let prepared = client_world::ingestion::prepare_block_mutations(
            vec![super::super::super::BlockMutationBatch {
                key,
                previous: worker.authority.terrain().sub_chunk(key),
                updates: updates.clone(),
            }],
            &worker.decode_ids(0),
        )
        .unwrap();
        assert_eq!(
            prepared.relight.contains(&key),
            relights,
            "case {previous_id:?}/{next_id}/{layer}"
        );
        let direct_mutation = ChunkStore::prepare_sub_chunk_blocks(
            key,
            direct.authority.terrain().sub_chunk(key).as_deref(),
            &updates,
            direct.classifier.air_network_id(),
        )
        .unwrap();
        assert!(direct.commit_block_mutations(vec![direct_mutation]));
        assert!(worker.commit_block_mutations_with_relight(prepared.mutations, &prepared.relight));
        assert_eq!(
            direct.authority.terrain().sub_chunk(key),
            worker.authority.terrain().sub_chunk(key)
        );
        assert_eq!(
            direct.lighting.jobs.pending.keys().collect::<BTreeSet<_>>(),
            worker.lighting.jobs.pending.keys().collect::<BTreeSet<_>>()
        );
        assert_eq!(
            direct.mesh_jobs.pending.keys().collect::<BTreeSet<_>>(),
            worker.mesh_jobs.pending.keys().collect::<BTreeSet<_>>()
        );
    }
}

/// A queued packet burst prepares one replacement and keeps every actor transition ordered.
#[test]
fn synced_packet_burst_shares_one_worker_and_one_mesh_fence() {
    use client_world::ingestion::{ActorBlockSyncMessage, SyncedBlockUpdateEvent};
    let mut stream = lit_stream(0);
    stream.set_custom_block_ids(4..5);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(2))
        .unwrap();
    stream.resident.insert(key);
    stream.poll_deadline = Some(Instant::now());
    for sequence in 1..=3 {
        let events = (0..128)
            .map(|cell| SyncedBlockUpdateEvent {
                update: BlockUpdateEvent {
                    dimension: 0,
                    position: [(cell >> 8), cell & 15, (cell >> 4) & 15],
                    layer: 0,
                    network_id: if sequence % 2 == 1 { 4 } else { 2 },
                },
                flags: 2,
                sync: ActorBlockSyncMessage {
                    actor_unique_id: sequence as i64,
                    message: 1,
                },
            })
            .collect();
        stream
            .submit(sequence, WorldEvent::SyncedBlockUpdates(events))
            .unwrap();
    }
    assert!(stream.pending_decode.is_empty());
    stream.poll_deadline = None;
    stream.polling = true;
    stream.apply_ready();
    assert_eq!(stream.pending_decode.len(), 1);
    assert_eq!(stream.order.heavy_count(), 3);
    let job = stream.pending_decode.pop_front().unwrap();
    stream.accept_decode_completion(job.job.run(job.queued_at));
    assert_eq!(stream.committed_sequence(), 3);
    assert_eq!(stream.order.heavy_count(), 0);
    assert_eq!(
        stream
            .authority
            .terrain()
            .sub_chunk(key)
            .unwrap()
            .runtime_id(0, 0, 0, 0),
        Some(4)
    );
    assert!(stream.lighting.jobs.pending.is_empty());
    assert_eq!(stream.mesh_jobs.pending.len(), 1);
    let fences = stream.actor_block_sync_fences();
    assert_eq!(fences.len(), 3 * 128);
    for (index, fence) in fences.iter().enumerate() {
        assert_eq!(fence.key, key);
        assert_eq!(fence.generation, fences[0].generation);
        assert_eq!(fence.sync.actor_unique_id, (index / 128 + 1) as i64);
    }
}
