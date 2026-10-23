use super::*;

#[test]
fn batch_preparation_walks_the_ceiling_once_for_the_whole_column() {
    for height in [4, 24, MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS as i32] {
        let mut stream = lit_stream(0);
        let keys: Vec<_> = (0..height)
            .rev()
            .map(|y| SubChunkKey::new(0, 0, y, 0))
            .collect();
        for &key in &keys {
            stream.record_known_air(key);
        }
        stream.mark_light_changed_sources(keys.iter().copied());
        lighting::LIGHT_COLUMN_CEILING_VISITS.with(|visits| visits.set(0));
        assert_eq!(stream.dispatch_light_jobs([8.0; 3], 1), height as usize);
        let visits = lighting::LIGHT_COLUMN_CEILING_VISITS.with(|visits| visits.get());
        for _ in &keys {
            stream
                .lighting
                .rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        }
        assert_eq!(visits, height as usize, "height={height}");
    }
}

/// Captures the independent section inputs used before batch ceiling reuse.
fn independent_jobs(stream: &WorldStream, keys: &[SubChunkKey]) -> Vec<PreparedLightJob> {
    keys.iter()
        .map(|&key| {
            let pending = stream.lighting.jobs.pending[&key];
            PreparedLightJob {
                key,
                identity: LightJobIdentity {
                    revision: pending.revision,
                    block_generation: stream.lighting.block_generations[&key],
                    previous_light_generation: stream
                        .lighting
                        .store
                        .light(key)
                        .map(|light| light.generation()),
                    batch_id: stream.lighting.next_batch_id + 1,
                    urgent: keys
                        .iter()
                        .any(|key| stream.lighting.jobs.pending[key].urgent),
                },
                blocks: stream.light_block_snapshot(key),
                prior: stream.light_prior_snapshot(key),
                bounds: light_bounds(key).unwrap(),
                queued_at: pending.queued_at,
                changed: None,
            }
        })
        .collect()
}

/// Compares every published light value and all output metadata with independent capture.
fn assert_batch_matches_independent(stream: &mut WorldStream, keys: &[SubChunkKey]) {
    let expected = solve_prepared_light_batch(independent_jobs(stream, keys));
    let camera = [8.0, keys[0].y as f32 * 16.0 + 8.0, 8.0];
    assert_eq!(stream.dispatch_light_jobs(camera, 1), keys.len());
    for _ in keys {
        let actual = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let expected = expected
            .iter()
            .find(|entry| entry.key == actual.key)
            .unwrap();
        assert_eq!(actual.identity, expected.identity);
        let actual = actual.result.unwrap();
        let expected = expected.result.as_ref().unwrap();
        assert!(light_levels_equal(
            &actual.replacement,
            &expected.replacement
        ));
        assert_eq!(
            actual.replacement.generation(),
            expected.replacement.generation()
        );
        assert_eq!(actual.direct_sky, expected.direct_sky);
        assert_eq!(
            actual.used_uniform_fast_path,
            expected.used_uniform_fast_path
        );
        assert_eq!(actual.light_levels_changed, expected.light_levels_changed);
        assert_eq!(actual.direct_sky_changed, expected.direct_sky_changed);
        assert_eq!(actual.changed_faces, expected.changed_faces);
    }
}

#[test]
fn batch_preparation_preserves_mixed_roofs_emitters_and_partial_columns() {
    let range = vanilla_dimension_range(0).unwrap();
    let declared_top = range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1;
    for (dimension, top) in [
        (0, declared_top),
        (0, 10),
        (0, declared_top + 21),
        (1, 7),
        (2, 7),
    ] {
        for roof in [false, true] {
            let mut stream = lit_stream(dimension);
            let keys: Vec<_> = (0..4)
                .map(|offset| SubChunkKey::new(dimension, 0, top - offset, 0))
                .collect();
            for &key in &keys {
                stream.record_known_air(key);
            }
            for (key, id) in [(keys[1], if roof { 2 } else { 0 }), (keys[3], 3)] {
                stream
                    .authority
                    .commit_sub_chunk(key, super::uniform_sub_chunk(id))
                    .unwrap();
                stream.sync_resident(key);
            }
            stream
                .authority
                .update_block(keys[2], BlockUpdate::new(8, 15, 8, 0, 1), 0)
                .unwrap();
            stream.sync_resident(keys[2]);
            let side = SubChunkKey::new(dimension, 1, top - 1, 0);
            stream.mark_light_changed_sources(keys.iter().copied());
            install_current_light(&mut stream, side, 7, 15, false);
            stream
                .lighting
                .jobs
                .pending
                .get_mut(&keys[3])
                .unwrap()
                .urgent = true;
            assert_batch_matches_independent(&mut stream, &keys);
        }
    }
}

#[test]
fn batch_preparation_keeps_waiters_for_stale_side_light() {
    let range = vanilla_dimension_range(0).unwrap();
    let declared_top = range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1;
    for top in [10, declared_top, declared_top + 21] {
        let mut stream = lit_stream(0);
        let keys: Vec<_> = (0..4)
            .map(|offset| SubChunkKey::new(0, 0, top - offset, 0))
            .collect();
        let mut sides = Vec::new();
        for &key in &keys {
            stream.record_known_air(key);
            let side = SubChunkKey::new(0, 1, key.y, 0);
            install_current_light(&mut stream, side, 0, 15, false);
            stream.mark_light_dirty_exact(side).unwrap();
            stream.lighting.jobs.pending.remove(&side);
            sides.push(side);
        }
        stream.mark_light_changed_sources(keys.iter().copied());
        for side in &sides {
            stream.lighting.jobs.pending.remove(side);
        }
        // Independent capture is the output oracle even when boundaries are untrusted.
        let expected = solve_prepared_light_batch(independent_jobs(&stream, &keys));
        lighting::LIGHT_COLUMN_CEILING_VISITS.with(|visits| visits.set(0));
        assert_eq!(stream.dispatch_light_jobs([8.0; 3], 1), keys.len());
        let visits = lighting::LIGHT_COLUMN_CEILING_VISITS.with(|visits| visits.get());
        for _ in &keys {
            let actual = stream
                .lighting
                .rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            let expected = expected
                .iter()
                .find(|entry| entry.key == actual.key)
                .unwrap()
                .result
                .as_ref()
                .unwrap();
            let actual = actual.result.unwrap();
            assert!(light_levels_equal(
                &actual.replacement,
                &expected.replacement
            ));
            assert_eq!(actual.direct_sky, expected.direct_sky);
        }
        assert_eq!(visits, keys.len(), "waiter ceiling work at top={top}");
        for &key in &keys {
            for &side in &sides {
                let expected = key.mesh_dependents().any(|neighbour| neighbour == side)
                    && !(key.y == top && top >= declared_top);
                assert_eq!(
                    stream
                        .lighting
                        .waiters
                        .get(&side)
                        .is_some_and(|waiters| waiters.contains(&key)),
                    expected,
                    "key={key:?} side={side:?}"
                );
            }
        }
    }
}

#[test]
fn batch_preparation_uses_resident_ceiling_beyond_selected_partial_batch() {
    let mut stream = lit_stream(0);
    let range = vanilla_dimension_range(0).unwrap();
    let declared_top = range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1;
    let keys: Vec<_> = (0..3)
        .map(|offset| SubChunkKey::new(0, 0, declared_top + 10 - offset, 0))
        .collect();
    for &key in &keys {
        stream.record_known_air(key);
    }
    // The missing section above the selected batch must remain unknown below a taller resident.
    let higher = SubChunkKey::new(0, 0, keys[0].y + 2, 0);
    install_current_light(&mut stream, higher, 0, 0, false);
    stream.mark_light_changed_sources(keys.iter().copied());
    assert_eq!(
        stream.light_block_snapshot(keys[0]).overworld_top_y,
        Some(higher.y * 16 + 15)
    );
    assert_batch_matches_independent(&mut stream, &keys);
}

#[test]
fn batch_preparation_keeps_member_identity_checks_after_dispatch() {
    for invalidated_field in 0..3 {
        let mut stream = lit_stream(0);
        let keys: Vec<_> = (0..3).map(|y| SubChunkKey::new(0, 0, y, 0)).rev().collect();
        for &key in &keys {
            install_current_light(&mut stream, key, 0, 0, false);
        }
        stream.mark_light_changed_sources(keys.iter().copied());
        assert_eq!(stream.dispatch_light_jobs([8.0; 3], 1), keys.len());
        let completions: Vec<_> = keys
            .iter()
            .map(|_| {
                stream
                    .lighting
                    .rx
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect();
        let target = keys[1];
        match invalidated_field {
            0 => {
                stream.mark_light_dirty_exact(target).unwrap();
            }
            1 => {
                *stream.lighting.block_generations.get_mut(&target).unwrap() += 1;
            }
            _ => {
                let generation = stream.lighting.store.light(target).unwrap().generation() + 1;
                stream
                    .lighting
                    .store
                    .insert_known_air(target, SubChunkLight::uniform(3, 0, generation).unwrap());
            }
        }
        let retained = Arc::clone(stream.lighting.store.light(target).unwrap());
        let stale_before = stream.stats.stale_light_jobs;
        for completion in completions {
            stream.accept_light_completion(completion);
        }
        assert!(Arc::ptr_eq(
            stream.lighting.store.light(target).unwrap(),
            &retained
        ));
        assert_eq!(stream.stats.stale_light_jobs, stale_before + 1);
    }
}
