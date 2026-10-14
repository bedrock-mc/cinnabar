//! Light changes: a column whose light a light job changes after its capture is sampled
//! again, though the content hash leaves light values out.

use world::SubChunkLight;

use super::*;

/// Completes a light job that changes chunk `(x, 0)`'s sub-chunk to uniform sky light
/// `sky`, as a neighbour's arrival or a torch across the border does.
fn relight(stream: &mut WorldStream, x: i32, sky: u8) {
    let key = SubChunkKey::new(0, x, SUB_CHUNK_Y, 0);
    let revision = stream.mark_light_dirty_exact(key).expect("lit key");
    let identity = LightJobIdentity {
        revision,
        block_generation: stream.lighting.block_generations[&key],
        previous_light_generation: stream
            .lighting
            .store
            .light(key)
            .map(|light| light.generation()),
        batch_id: 0,
        urgent: false,
    };
    stream.lighting.jobs.pending.remove(&key);
    stream.lighting.jobs.in_flight.insert(key, identity);
    stream.accept_light_completion(LightCompletion {
        key,
        identity,
        result: Ok(SolvedLightJob {
            replacement: SubChunkLight::uniform(0, sky, revision).expect("valid light"),
            direct_sky: Arc::new(DirectSkyMask::Uniform(false)),
            used_uniform_fast_path: false,
            light_levels_changed: true,
            direct_sky_changed: false,
            changed_faces: [false; 6],
        }),
        queue_wait: Duration::ZERO,
        duration: Duration::from_millis(1),
    });
    assert!(stream.light_is_current(key));
}

#[test]
fn a_column_relit_after_its_capture_is_sampled_again() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);
    assert_eq!(chunk_one_face_sky(&store), 15);

    relight(&mut stream, 1, 4);
    assert_eq!(
        stats(&stream).dirty_columns,
        1,
        "the light change waits to settle"
    );
    settle(&mut stream);
    wait_for_merges(&stream, 2);
    assert_eq!(stats(&stream).skipped_unchanged, 0);
    assert_eq!(chunk_one_face_sky(&store), 4);
}

#[test]
fn light_solved_before_a_columns_first_capture_keeps_a_revisit_skipping() {
    let store = TestStore::new();
    let mut stream = stream();
    let _link = enable(&mut stream, &store);
    commit_column(&mut stream, 1, 1, STONE, 15);
    settle(&mut stream);
    wait_for_merges(&stream, 1);

    // A revisit loads the column and lights it before its first capture.
    let mut rejoined = self::stream();
    let _rejoined_link = enable(&mut rejoined, &store);
    commit_column(&mut rejoined, 1, 1, STONE, 15);
    relight(&mut rejoined, 1, 4);
    settle(&mut rejoined);
    eventually("the hash check", || {
        let stats = stats(&rejoined);
        stats.skipped_unchanged == 1 && stats.is_settled()
    });
    assert_eq!(stats(&rejoined).sampled, 0);
}

#[test]
fn light_changes_queue_nothing_while_ingestion_is_off() {
    let mut stream = stream();
    commit_column(&mut stream, 1, 1, STONE, 15);
    relight(&mut stream, 1, 4);
    settle(&mut stream);
    assert_eq!(stream.distant_terrain_stats(), None);
}
