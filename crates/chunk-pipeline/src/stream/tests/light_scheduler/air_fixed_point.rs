use super::*;

const AIR_COLUMNS: [[i32; 2]; 9] = [
    [0, 0],
    [-1, 0],
    [0, -1],
    [1, 0],
    [0, 1],
    [-1, -1],
    [-1, 1],
    [1, -1],
    [1, 1],
];

fn column_keys(x: i32, z: i32) -> Vec<SubChunkKey> {
    let range = vanilla_dimension_range(0).unwrap();
    (0..range.sub_chunk_count)
        .map(|offset| SubChunkKey::new(0, x, range.base_sub_chunk_y + offset as i32, z))
        .collect()
}

fn settle(stream: &mut WorldStream) {
    for _ in 0..2_048 {
        stream.dispatch_light_jobs([8.0, 80.0, 8.0], usize::MAX);
        if stream.lighting.jobs.pending.is_empty() && stream.lighting.jobs.in_flight.is_empty() {
            return;
        }
        if !stream.lighting.jobs.in_flight.is_empty() {
            let completion = stream
                .lighting
                .rx
                .recv_timeout(Duration::from_secs(5))
                .expect("lighting must make progress");
            stream.accept_light_completion(completion);
            while let Ok(completion) = stream.lighting.rx.try_recv() {
                stream.accept_light_completion(completion);
            }
        } else {
            std::thread::yield_now();
        }
    }
    panic!("incremental air lighting did not settle");
}

fn assert_full_solve(stream: &WorldStream, keys: &[SubChunkKey]) {
    let mut blocks = stream.light_block_snapshot(keys[0]);
    let mut prior = stream.light_prior_snapshot(keys[0]);
    for &key in &keys[1..] {
        blocks
            .blocks
            .extend(stream.light_block_snapshot(key).blocks);
        let next = stream.light_prior_snapshot(key);
        prior.light.extend(next.light);
        prior.direct_sky.extend(next.direct_sky);
        prior.trusted_boundaries.extend(next.trusted_boundaries);
    }
    blocks.resolve_palette_light();
    let bounds = keys.iter().map(|&key| light_bounds(key).unwrap());
    let min = bounds
        .clone()
        .fold(BlockPos::new(i32::MAX, i32::MAX, i32::MAX), |min, b| {
            let p = b.min();
            BlockPos::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z))
        });
    let max = bounds.fold(BlockPos::new(i32::MIN, i32::MIN, i32::MIN), |max, b| {
        let p = b.max();
        BlockPos::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z))
    });
    let volume = usize::try_from(max.x - min.x + 1).unwrap()
        * usize::try_from(max.y - min.y + 1).unwrap()
        * usize::try_from(max.z - min.z + 1).unwrap();
    let solved = solve_light(
        &blocks,
        &prior,
        LightBounds::new(0, min, max).unwrap(),
        1,
        blocks.profile,
        SolverLimits::new(volume, volume * MAX_LIGHT_COLUMN_BATCH_SUB_CHUNKS),
    )
    .unwrap();
    for &key in keys {
        assert!(stream.light_is_current(key), "{key:?}");
        if !light_levels_equal(
            stream.lighting.store.light(key).unwrap(),
            &solved.sub_chunks()[&key],
        ) {
            let actual = stream.lighting.store.light(key).unwrap();
            let oracle = &solved.sub_chunks()[&key];
            let mismatch = (0_u8..16)
                .flat_map(|x| (0_u8..16).flat_map(move |z| (0_u8..16).map(move |y| [x, y, z])))
                .find(|p| {
                    [LightChannel::Block, LightChannel::Sky]
                        .into_iter()
                        .any(|channel| {
                            actual.get(channel, p[0], p[1], p[2])
                                != oracle.get(channel, p[0], p[1], p[2])
                        })
                })
                .unwrap();
            eprintln!(
                "full_solve_mismatch key={key:?} sample={mismatch:?} actual_block={:?} expected_block={:?} actual_sky={:?} expected_sky={:?}",
                actual.get(LightChannel::Block, mismatch[0], mismatch[1], mismatch[2]),
                oracle.get(LightChannel::Block, mismatch[0], mismatch[1], mismatch[2]),
                actual.get(LightChannel::Sky, mismatch[0], mismatch[1], mismatch[2]),
                oracle.get(LightChannel::Sky, mismatch[0], mismatch[1], mismatch[2])
            );
        }
        assert!(
            light_levels_equal(
                stream.lighting.store.light(key).unwrap(),
                &solved.sub_chunks()[&key]
            ),
            "{key:?}"
        );
        assert_eq!(
            stream.lighting.direct_sky[&key].mask.as_ref(),
            &DirectSkyMask::from_output(&solved, key),
            "{key:?}"
        );
    }
}

fn incremental_air(explicit: bool) -> (WorldStream, Vec<SubChunkKey>) {
    incremental_air_with_neighbor_refresh(explicit, false)
}

fn incremental_air_with_neighbor_refresh(
    explicit: bool,
    refresh_neighbors: bool,
) -> (WorldStream, Vec<SubChunkKey>) {
    let mut stream = lit_stream(0);
    let mut keys = Vec::new();
    for [x, z] in AIR_COLUMNS {
        let column = column_keys(x, z);
        for &key in &column {
            if explicit {
                stream
                    .authority
                    .commit_sub_chunk(key, super::uniform_sub_chunk(0))
                    .unwrap();
                stream.sync_resident(key);
            } else {
                stream.record_known_air(key);
            }
        }
        stream.mark_light_changed_sources(column.iter().copied());
        if refresh_neighbors {
            for key in column
                .iter()
                .copied()
                .flat_map(SubChunkKey::mesh_dependents)
                .collect::<BTreeSet<_>>()
            {
                stream.mark_light_dirty_exact(key);
            }
        }
        keys.extend(column);
        settle(&mut stream);
    }
    (stream, keys)
}

#[test]
fn incremental_air_columns_do_not_resolve_unchanged_sky_again() {
    for explicit in [false, true] {
        let started = Instant::now();
        let (stream, keys) = incremental_air(explicit);
        let stats = stream.stats();
        eprintln!(
            "incremental_air explicit={explicit} columns=9 sections={} accepted={} changed={} noop={} stale={} diagnostic_us={}",
            keys.len(),
            stats.accepted_light_jobs,
            stats.value_changed_light_jobs,
            stats.noop_light_jobs,
            stats.stale_light_jobs,
            started.elapsed().as_micros()
        );
        assert_full_solve(&stream, &keys);
        assert_eq!(
            stats.noop_light_jobs, 0,
            "neighboring air arrived without changing solved light"
        );
        assert_eq!(stats.accepted_light_jobs, keys.len() as u64);
        assert_eq!(stats.value_changed_light_jobs, keys.len() as u64);
        assert_eq!(stats.stale_light_jobs, 0);
    }
}

#[test]
fn air_arrival_preserves_current_empty_target_values_and_provenance_identity() {
    for explicit in [false, true] {
        let mut stream = lit_stream(0);
        let target = SubChunkKey::new(0, 0, 19, 0);
        let source = SubChunkKey::new(0, 1, 19, 0);
        if explicit {
            stream
                .authority
                .commit_sub_chunk(target, super::uniform_sub_chunk(0))
                .unwrap();
        }
        install_current_light(&mut stream, target, 0, 15, true);
        let light = Arc::clone(stream.lighting.store.light(target).unwrap());
        let direct = Arc::clone(&stream.lighting.direct_sky[&target].mask);
        let ownership = stream.lighting.ownership[&target];
        stream.record_known_air(source);
        stream.mark_light_changed_sources([source]);
        assert!(stream.light_is_current(target));
        assert!(!stream.lighting.jobs.pending.contains_key(&target));
        assert!(stream.lighting.jobs.pending.contains_key(&source));
        settle(&mut stream);
        assert_eq!(stream.lighting.ownership[&target], ownership);
        assert!(Arc::ptr_eq(
            &light,
            stream.lighting.store.light(target).unwrap()
        ));
        assert!(Arc::ptr_eq(
            &direct,
            &stream.lighting.direct_sky[&target].mask
        ));
        assert_full_solve(&stream, &[target, source]);
    }
}

#[test]
fn air_fixed_point_keeps_luminous_arrival_removal_and_roof_changes_exact() {
    let (mut stream, keys) = incremental_air(false);
    let source = SubChunkKey::new(0, 0, 5, 0);
    let side = SubChunkKey::new(0, 1, 5, 0);
    for id in [1, 0, 2, 0] {
        stream
            .authority
            .commit_sub_chunk(source, super::uniform_sub_chunk(id))
            .unwrap();
        stream.sync_resident(source);
        stream.mark_changed(source, Instant::now());
        assert!(stream.lighting.jobs.pending.contains_key(&source));
        if id != 0 {
            assert!(stream.lighting.jobs.pending.contains_key(&side));
        }
        settle(&mut stream);
        assert_full_solve(&stream, &keys);
        if id == 1 {
            assert_eq!(
                stream
                    .lighting
                    .store
                    .light(side)
                    .unwrap()
                    .get(LightChannel::Block, 0, 8, 8),
                Some(14)
            );
        }
    }
}

#[test]
fn air_fixed_point_rejects_noncurrent_nonuniform_and_retained_luminous_inputs() {
    for case in 0..19 {
        let mut stream = lit_stream(0);
        let target = SubChunkKey::new(0, 0, 19, 0);
        let source = SubChunkKey::new(0, 1, 19, 0);
        install_current_light(&mut stream, target, 0, 15, true);
        match case {
            0 => {
                stream.lighting.ownership.remove(&target);
            }
            1 => {
                stream.mark_light_dirty_exact(target).unwrap();
            }
            2 => {
                let revision = stream.mark_light_dirty_exact(target).unwrap();
                stream.lighting.jobs.pending.remove(&target);
                stream.lighting.jobs.in_flight.insert(
                    target,
                    LightJobIdentity {
                        revision,
                        block_generation: stream.lighting.block_generations[&target],
                        previous_light_generation: Some(
                            stream.lighting.store.light(target).unwrap().generation(),
                        ),
                        batch_id: 0,
                        urgent: false,
                    },
                );
            }
            3 => {
                stream
                    .lighting
                    .direct_sky
                    .get_mut(&target)
                    .unwrap()
                    .light_revision += 1;
            }
            4 => {
                stream.lighting.direct_sky.get_mut(&target).unwrap().mask =
                    Arc::new(DirectSkyMask::Packed(Box::new([u64::MAX; 64])));
            }
            5 => {
                install_current_light(&mut stream, target, 1, 15, true);
            }
            6 => {
                let mut light = stream
                    .lighting
                    .store
                    .light(target)
                    .unwrap()
                    .as_ref()
                    .clone();
                light.set(LightChannel::Sky, 8, 8, 8, 14).unwrap();
                stream
                    .lighting
                    .store
                    .commit_if_generation(target, Some(light.generation()), light);
            }
            7 => {
                install_current_light(&mut stream, source, 1, 15, true);
            }
            8 => {
                install_current_light(&mut stream, source, 0, 15, true);
                stream
                    .lighting
                    .direct_sky
                    .get_mut(&source)
                    .unwrap()
                    .light_revision += 1;
            }
            9 => {
                stream
                    .authority
                    .commit_sub_chunk(source, super::uniform_sub_chunk(2))
                    .unwrap();
                stream.sync_resident(source);
            }
            10 => {
                install_current_light(&mut stream, source, 0, 15, true);
                stream.mark_light_dirty_exact(source).unwrap();
            }
            11 => {
                install_current_light(&mut stream, source, 0, 15, true);
                let revision = stream.mark_light_dirty_exact(source).unwrap();
                stream.lighting.jobs.pending.remove(&source);
                stream.lighting.jobs.in_flight.insert(
                    source,
                    LightJobIdentity {
                        revision,
                        block_generation: stream.lighting.block_generations[&source],
                        previous_light_generation: Some(
                            stream.lighting.store.light(source).unwrap().generation(),
                        ),
                        batch_id: 0,
                        urgent: false,
                    },
                );
            }
            12 => {
                install_current_light(&mut stream, source, 0, 15, true);
                let mut light = stream
                    .lighting
                    .store
                    .light(source)
                    .unwrap()
                    .as_ref()
                    .clone();
                light.set(LightChannel::Block, 8, 8, 8, 1).unwrap();
                stream
                    .lighting
                    .store
                    .commit_if_generation(source, Some(light.generation()), light);
            }
            13 => {
                install_current_light(&mut stream, source, 0, 15, true);
                stream.lighting.direct_sky.get_mut(&source).unwrap().mask =
                    Arc::new(DirectSkyMask::Packed(Box::new([u64::MAX; 64])));
            }
            14 => {}
            15 => {
                stream
                    .authority
                    .commit_sub_chunk(target, super::uniform_sub_chunk(3))
                    .unwrap();
                stream.sync_resident(target);
            }
            16 => {
                let sub_chunk =
                    SubChunk::decode(&[8, 2, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
                stream
                    .authority
                    .commit_sub_chunk(source, sub_chunk)
                    .unwrap();
                stream.sync_resident(source);
            }
            17 => {
                install_current_light(&mut stream, source, 0, 15, true);
                let mut light = stream
                    .lighting
                    .store
                    .light(source)
                    .unwrap()
                    .as_ref()
                    .clone();
                light.set(LightChannel::Sky, 8, 8, 8, 14).unwrap();
                stream
                    .lighting
                    .store
                    .commit_if_generation(source, Some(light.generation()), light);
            }
            18 => {
                install_current_light(&mut stream, source, 0, 15, true);
                stream
                    .lighting
                    .ownership
                    .get_mut(&source)
                    .unwrap()
                    .light_revision += 1;
            }
            _ => unreachable!(),
        }
        if ![9, 14, 16].contains(&case) {
            stream.record_known_air(source);
        }
        stream.mark_light_changed_sources([source]);
        assert!(
            stream.lighting.jobs.pending.contains_key(&target),
            "fallback case {case}"
        );
        if case != 14 {
            assert!(stream.lighting.jobs.pending.contains_key(&source));
        }
    }
}

#[test]
fn removing_roof_restores_direct_sky_below_the_initial_invalidations() {
    let mut stream = lit_stream(0);
    let keys = (16..=19)
        .map(|y| SubChunkKey::new(0, 0, y, 0))
        .collect::<Vec<_>>();
    let roof = keys[2];
    for &key in &keys {
        stream.record_known_air(key);
    }
    stream
        .authority
        .commit_sub_chunk(roof, super::uniform_sub_chunk(2))
        .unwrap();
    stream.sync_resident(roof);
    stream.mark_light_changed_sources(keys.iter().copied());
    settle(&mut stream);
    assert_eq!(
        stream
            .lighting
            .store
            .light(keys[0])
            .unwrap()
            .get(LightChannel::Sky, 8, 8, 8),
        Some(0)
    );
    assert_full_solve(&stream, &keys);

    stream
        .authority
        .commit_sub_chunk(roof, super::uniform_sub_chunk(0))
        .unwrap();
    stream.sync_resident(roof);
    stream.mark_changed(roof, Instant::now());
    settle(&mut stream);
    assert_eq!(
        stream
            .lighting
            .store
            .light(keys[0])
            .unwrap()
            .get(LightChannel::Sky, 8, 8, 8),
        Some(15),
        "removing the roof must restore direct sky beyond the first lower section"
    );
    assert_full_solve(&stream, &keys);
}

#[test]
fn later_air_source_light_publication_still_requeues_preserved_target() {
    for explicit in [false, true] {
        let mut stream = lit_stream(0);
        let target_column = column_keys(0, 0);
        for &key in &target_column {
            stream.record_known_air(key);
        }
        stream.mark_light_changed_sources(target_column.iter().copied());
        settle(&mut stream);
        let target = SubChunkKey::new(0, 0, 5, 0);
        let target_revision = stream.lighting.ownership[&target].light_revision;

        let diagonal = column_keys(1, 1);
        let emitter = SubChunkKey::new(0, 1, 5, 1);
        for &key in &diagonal {
            stream.record_known_air(key);
        }
        stream
            .authority
            .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
            .unwrap();
        stream.sync_resident(emitter);
        stream.mark_light_changed_sources(diagonal.iter().copied());
        settle(&mut stream);
        let mut keys = target_column;
        keys.extend(diagonal);
        assert_full_solve(&stream, &keys);
        assert_eq!(
            stream
                .lighting
                .store
                .light(target)
                .unwrap()
                .get(LightChannel::Block, 15, 8, 15),
            Some(0)
        );

        let arriving = column_keys(1, 0);
        for &key in &arriving {
            if explicit {
                stream
                    .authority
                    .commit_sub_chunk(key, super::uniform_sub_chunk(0))
                    .unwrap();
                stream.sync_resident(key);
            } else {
                stream.record_known_air(key);
            }
        }
        stream.mark_light_changed_sources(arriving.iter().copied());
        assert!(stream.light_is_current(target));
        assert!(!stream.lighting.jobs.pending.contains_key(&target));
        assert_eq!(
            stream.lighting.ownership[&target].light_revision,
            target_revision
        );
        settle(&mut stream);
        keys.extend(arriving);
        assert_full_solve(&stream, &keys);
        assert_eq!(
            stream
                .lighting
                .store
                .light(target)
                .unwrap()
                .get(LightChannel::Block, 15, 8, 15),
            Some(13)
        );
        assert_ne!(
            stream.lighting.ownership[&target].light_revision,
            target_revision
        );
    }
}

#[test]
fn mixed_changed_source_sets_preserve_non_air_face_invalidation() {
    let mut stream = lit_stream(0);
    let target = SubChunkKey::new(0, 0, 19, 0);
    let air = SubChunkKey::new(0, 1, 19, 0);
    let emitter = SubChunkKey::new(0, -1, 19, 0);
    install_current_light(&mut stream, target, 0, 15, true);
    stream.record_known_air(air);
    stream
        .authority
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.sync_resident(emitter);
    stream.mark_light_changed_sources([air, emitter]);
    assert!(stream.lighting.jobs.pending.contains_key(&target));
    settle(&mut stream);
    assert_full_solve(&stream, &[air, target, emitter]);
}

#[test]
fn incremental_air_neighbor_refresh_timing() {
    if std::env::var_os("CINNABAR_JOIN_CACHE_BENCH").is_none() {
        eprintln!(
            "missing fixture: CINNABAR_JOIN_CACHE_BENCH=1 for incremental air neighbor timing"
        );
        return;
    }
    for explicit in [false, true] {
        let mut redundant = Vec::new();
        let mut minimal = Vec::new();
        let neighbor_interfaces = AIR_COLUMNS
            .iter()
            .enumerate()
            .map(|(i, [x, z])| {
                AIR_COLUMNS[..i]
                    .iter()
                    .filter(|[sx, sz]| (x - sx).abs() + (z - sz).abs() == 1)
                    .count()
            })
            .sum::<usize>();
        let sections = column_keys(0, 0).len() * AIR_COLUMNS.len();
        let redundant_jobs = neighbor_interfaces * column_keys(0, 0).len();
        for trial in 0..11 {
            for refresh_neighbors in if trial & 1 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let started = Instant::now();
                let (stream, keys) =
                    incremental_air_with_neighbor_refresh(explicit, refresh_neighbors);
                let elapsed = started.elapsed();
                let stats = stream.stats();
                assert_eq!(stats.value_changed_light_jobs, keys.len() as u64);
                assert_eq!(stats.stale_light_jobs, 0);
                if refresh_neighbors {
                    assert_eq!(
                        stats.accepted_light_jobs,
                        (sections + redundant_jobs) as u64
                    );
                    assert_eq!(stats.noop_light_jobs, redundant_jobs as u64);
                    redundant.push(elapsed);
                } else {
                    assert_eq!(stats.accepted_light_jobs, sections as u64);
                    assert_eq!(stats.noop_light_jobs, 0);
                    minimal.push(elapsed);
                }
                if trial == 0 {
                    assert_full_solve(&stream, &keys);
                }
            }
        }
        redundant.sort_unstable();
        minimal.sort_unstable();
        eprintln!(
            "incremental_air_neighbor_refresh explicit={explicit} sections={sections} forced_accepted={} forced_noop={redundant_jobs} minimal_accepted={sections} minimal_noop=0 forced_p50_us={} minimal_p50_us={}",
            sections + redundant_jobs,
            redundant[5].as_micros(),
            minimal[5].as_micros()
        );
    }
}
