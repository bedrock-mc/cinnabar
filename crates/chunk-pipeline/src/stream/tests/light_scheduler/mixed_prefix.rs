use super::*;
use std::hint::black_box;

fn column_jobs(edit: impl FnMut(&mut WorldStream)) -> Vec<PreparedLightJob> {
    let range = vanilla_dimension_range(0).unwrap();
    batch_jobs(
        (0..range.sub_chunk_count)
            .rev()
            .map(|offset| range.base_sub_chunk_y + offset as i32),
        edit,
    )
}

/// Column batch for the given section heights, highest first, as the scheduler orders it.
fn batch_jobs(
    heights: impl IntoIterator<Item = i32>,
    mut edit: impl FnMut(&mut WorldStream),
) -> Vec<PreparedLightJob> {
    let mut stream = lit_stream(0);
    let keys: Vec<_> = heights
        .into_iter()
        .map(|y| SubChunkKey::new(0, 0, y, 0))
        .collect();
    for key in &keys {
        stream.record_known_air(*key);
    }
    edit(&mut stream);
    stream.mark_light_changed_sources(keys.iter().copied());
    keys.into_iter()
        .map(|key| {
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
                        .map(|l| l.generation()),
                    batch_id: 1,
                    urgent: false,
                },
                blocks: stream.light_block_snapshot(key),
                prior: stream.light_prior_snapshot(key),
                bounds: light_bounds(key).unwrap(),
                queued_at: pending.queued_at,
            }
        })
        .collect()
}

fn resident(stream: &mut WorldStream, key: SubChunkKey, id: u32) {
    stream
        .authority
        .commit_sub_chunk(key, super::uniform_sub_chunk(id))
        .unwrap();
    stream.sync_resident(key);
}

fn full_column_oracle(jobs: &[PreparedLightJob]) -> LightSolveOutput {
    let mut blocks = jobs[0].blocks.clone();
    let mut prior = jobs[0].prior.clone();
    for job in &jobs[1..] {
        blocks.blocks.extend(job.blocks.blocks.clone());
        prior.light.extend(job.prior.light.clone());
        prior.direct_sky.extend(job.prior.direct_sky.clone());
        prior.trusted_boundaries.extend(
            job.prior
                .trusted_boundaries
                .iter()
                .map(|(&key, &value)| (key, value)),
        );
    }
    blocks.resolve_palette_light();
    let min = jobs.last().unwrap().bounds.min();
    let max = jobs[0].bounds.max();
    solve_light(
        &blocks,
        &prior,
        LightBounds::new(0, min, max).unwrap(),
        1,
        blocks.profile,
        LIGHT_COLUMN_SOLVE_LIMITS,
    )
    .unwrap()
}

fn compare_full_column(jobs: Vec<PreparedLightJob>) -> usize {
    let full = full_column_oracle(&jobs);
    let entries = solve_prepared_light_batch(jobs);
    let mut fast = 0;
    for entry in entries {
        let solved = entry.result.unwrap();
        fast += usize::from(solved.used_uniform_fast_path);
        assert!(
            light_levels_equal(&solved.replacement, &full.sub_chunks()[&entry.key]),
            "{:?} fast={}",
            entry.key,
            solved.used_uniform_fast_path
        );
        assert_eq!(
            solved.direct_sky.as_ref(),
            &DirectSkyMask::from_output(&full, entry.key),
            "{:?}",
            entry.key
        );
    }
    fast
}

#[test]
fn mixed_roof_column_skips_distant_upper_air_with_exact_full_column_output() {
    let roof = SubChunkKey::new(0, 0, 5, 0);
    let jobs = column_jobs(|stream| resident(stream, roof, 2));
    let expected = jobs.iter().filter(|job| job.key.y >= roof.y + 2).count();
    assert!(expected > 0);
    assert_eq!(compare_full_column(jobs), expected);
}

#[test]
fn mixed_prefix_keeps_emitting_top_voxel_and_its_upper_air_halo_dense() {
    let emitter = SubChunkKey::new(0, 0, 5, 0);
    let jobs = column_jobs(|stream| {
        stream
            .authority
            .update_block(emitter, BlockUpdate::new(8, 15, 8, 0, 1), 0)
            .unwrap();
        stream.sync_resident(emitter);
    });
    let expected = jobs.iter().filter(|job| job.key.y >= emitter.y + 2).count();
    assert_eq!(compare_full_column(jobs), expected);
}

#[test]
fn mixed_prefix_accounts_for_higher_side_emitters_and_nonuniform_retained_light() {
    let roof = SubChunkKey::new(0, 0, 5, 0);
    let side = SubChunkKey::new(0, 1, 13, 0);
    let mut jobs = column_jobs(|stream| {
        resident(stream, roof, 2);
        resident(stream, side, 1);
        install_current_light(stream, side, 15, 0, false);
    });
    for job in &mut jobs {
        job.prior.trusted_boundaries.insert(side, ());
    }
    let side_lit = full_column_oracle(&jobs);
    assert_eq!(
        side_lit.sub_chunks()[&SubChunkKey::new(0, 0, side.y, 0)].get(
            LightChannel::Block,
            15,
            8,
            8
        ),
        Some(14)
    );
    let expected = jobs.iter().filter(|job| job.key.y >= side.y + 2).count();
    assert_eq!(compare_full_column(jobs), expected);

    let retained = SubChunkKey::new(0, 0, 8, 0);
    let jobs = column_jobs(|stream| {
        resident(stream, roof, 2);
        install_current_light(stream, retained, 0, 0, false);
        let mut light = stream
            .lighting
            .store
            .light(retained)
            .unwrap()
            .as_ref()
            .clone();
        light.set(LightChannel::Block, 8, 8, 8, 7).unwrap();
        stream
            .lighting
            .store
            .commit_if_generation(retained, Some(light.generation()), light);
    });
    let expected = jobs
        .iter()
        .filter(|job| job.key.y >= retained.y + 2)
        .count();
    assert_eq!(compare_full_column(jobs), expected);
}

#[test]
fn mixed_prefix_preserves_packed_and_stale_sky_provenance() {
    for stale in [false, true] {
        let side = SubChunkKey::new(0, 1, 18, 0);
        let mut jobs = column_jobs(|stream| {
            resident(stream, SubChunkKey::new(0, 0, 5, 0), 2);
            install_current_light(stream, side, 0, 15, false);
            let mut words = Box::new([u64::MAX; 64]);
            let index = light_local_index(0, 8, 8);
            words[index / 64] &= !(1 << (index % 64));
            let direct = stream.lighting.direct_sky.get_mut(&side).unwrap();
            direct.mask = Arc::new(DirectSkyMask::Packed(words));
            direct.light_revision += u64::from(stale);
        });
        for job in &mut jobs {
            job.prior.trusted_boundaries.insert(side, ());
        }
        assert!(compare_full_column(jobs) > 0);
    }
}

fn resident_column_jobs(side_emitter: bool) -> Vec<PreparedLightJob> {
    column_jobs(|stream| {
        let range = vanilla_dimension_range(0).unwrap();
        for offset in 0..range.sub_chunk_count {
            resident(
                stream,
                SubChunkKey::new(0, 0, range.base_sub_chunk_y + offset as i32, 0),
                0,
            );
        }
        resident(stream, SubChunkKey::new(0, 0, 5, 0), 2);
        if side_emitter {
            let side = SubChunkKey::new(0, 1, 13, 0);
            resident(stream, side, 1);
            install_current_light(stream, side, 15, 0, false);
        }
    })
}

#[test]
fn resident_air_palettes_preserve_full_column_and_side_emission_output() {
    for side_emitter in [false, true] {
        let mut jobs = resident_column_jobs(side_emitter);
        let source_y = if side_emitter { 13 } else { 5 };
        if side_emitter {
            let side = SubChunkKey::new(0, 1, 13, 0);
            for job in &mut jobs {
                job.prior.trusted_boundaries.insert(side, ());
            }
        }
        assert!(jobs.iter().all(|job| matches!(
            job.blocks.blocks.get(&job.key).unwrap(),
            SnapshotBlock::Resident(_)
        )));
        let expected = jobs.iter().filter(|job| job.key.y >= source_y + 2).count();
        assert_eq!(compare_full_column(jobs), expected);
    }
}

#[test]
fn resident_air_proof_covers_all_layers_and_rejects_non_air_palettes() {
    let classifier = BlockClassifier::new(0);
    for (layers, expected) in [
        (vec![], true),
        (vec![0], true),
        (vec![0, 0], true),
        (vec![0, 1], false),
        (vec![2, 0], false),
        (vec![3], false),
    ] {
        let mut bytes = vec![8, layers.len() as u8];
        for id in layers {
            bytes.extend([1, id << 1]);
        }
        let chunk = SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 });
        let block = SnapshotBlock::Resident(Arc::new(chunk));
        assert_eq!(block.is_known_air(classifier), expected);
    }
}

#[test]
fn layered_resident_air_and_hidden_layer_emission_match_full_solver() {
    for layer_id in [0_u8, 1] {
        let jobs = column_jobs(|stream| {
            resident(stream, SubChunkKey::new(0, 0, 5, 0), 2);
            let layered = SubChunk::decode(
                &[8, 2, 1, 0, 1, layer_id << 1],
                &world::RawBlockIds { air: 0 },
            );
            stream
                .authority
                .commit_sub_chunk(SubChunkKey::new(0, 0, 14, 0), layered)
                .unwrap();
            stream.sync_resident(SubChunkKey::new(0, 0, 14, 0));
        });
        let source_y = if layer_id == 0 { 5 } else { 14 };
        let expected = jobs.iter().filter(|job| job.key.y >= source_y + 2).count();
        assert_eq!(compare_full_column(jobs), expected);
    }
}

#[test]
fn resident_air_proof_keeps_normalized_indices_and_unused_palette_values_conservative() {
    for (indices, palette, expected) in [
        (u32::MAX, vec![2_u8, 0], true),
        (0, vec![4_u8, 0, 2], false),
    ] {
        let mut bytes = vec![8, 1, 3];
        for _ in 0..world::BLOCKS_PER_SUB_CHUNK / u32::BITS as usize {
            bytes.extend(indices.to_le_bytes());
        }
        bytes.extend(palette);
        let chunk = SubChunk::decode(&bytes, &world::RawBlockIds { air: 0 });
        assert_eq!(chunk.runtime_id(0, 8, 8, 8), Some(0));
        let block = SnapshotBlock::Resident(Arc::new(chunk));
        assert_eq!(block.is_known_air(BlockClassifier::new(0)), expected);
    }
}

#[test]
fn resident_air_column_prefix_timing() {
    if std::env::var_os("CINNABAR_JOIN_CACHE_BENCH").is_none() {
        eprintln!("missing fixture: CINNABAR_JOIN_CACHE_BENCH=1 for resident-air prefix timing");
        return;
    }
    let jobs = resident_column_jobs(false);
    let sections = jobs.len();
    let fast = compare_full_column(jobs);
    let mut full = Vec::new();
    let mut prefix = Vec::new();
    for _ in 0..11 {
        let jobs = resident_column_jobs(false);
        let started = Instant::now();
        black_box(full_column_oracle(&jobs));
        full.push(started.elapsed().as_secs_f64() * 1000.0);
        let jobs = resident_column_jobs(false);
        let started = Instant::now();
        black_box(solve_prepared_light_batch(jobs));
        prefix.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    full.sort_by(f64::total_cmp);
    prefix.sort_by(f64::total_cmp);
    eprintln!(
        "resident_air_prefix sections={sections} dense_before={sections} dense_after={} full_solver_p50_ms={:.3} prefix_batch_p50_ms={:.3}",
        sections - fast,
        full[5],
        prefix[5]
    );
}

#[test]
fn mixed_tall_column_prefix_timing() {
    if std::env::var_os("CINNABAR_JOIN_CACHE_BENCH").is_none() {
        eprintln!("missing fixture: CINNABAR_JOIN_CACHE_BENCH=1 for mixed column prefix timing");
        return;
    }
    let make = || column_jobs(|stream| resident(stream, SubChunkKey::new(0, 0, 5, 0), 2));
    let jobs = make();
    let sections = jobs.len();
    let fast = compare_full_column(jobs);
    let mut full = Vec::new();
    let mut prefix = Vec::new();
    for _ in 0..11 {
        let jobs = make();
        let started = Instant::now();
        black_box(full_column_oracle(&jobs));
        full.push(started.elapsed().as_secs_f64() * 1000.0);
        let jobs = make();
        let started = Instant::now();
        black_box(solve_prepared_light_batch(jobs));
        prefix.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    full.sort_by(f64::total_cmp);
    prefix.sort_by(f64::total_cmp);
    eprintln!(
        "mixed_column_prefix sections={sections} dense_before={sections} dense_after={} full_solver_p50_ms={:.3} prefix_batch_p50_ms={:.3}",
        sections - fast,
        full[5],
        prefix[5]
    );
}

/// Skylight entering lower air from a side can rise into upper air, so peeling must not assume it dark.
#[test]
fn mixed_prefix_keeps_side_lit_air_below_dark_upper_air_dense() {
    let side = SubChunkKey::new(0, 1, 9, 0);
    let mut jobs = batch_jobs([10, 9, 8, 7], |stream| {
        resident(stream, SubChunkKey::new(0, 0, 7, 0), 2);
        install_current_light(stream, side, 0, 15, false);
    });
    for job in &mut jobs {
        job.prior.trusted_boundaries.insert(side, ());
    }
    let full = full_column_oracle(&jobs);
    assert_eq!(
        full.sub_chunks()[&SubChunkKey::new(0, 0, 10, 0)].get(LightChannel::Sky, 15, 0, 8),
        Some(13)
    );
    compare_full_column(jobs);
}

/// Random mixed batches: the prefix shortcut must match the full solver section for section.
#[test]
fn mixed_prefix_matches_the_full_solver_on_random_batches() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % bound
    };
    let range = vanilla_dimension_range(0).unwrap();
    let highest = range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1;
    let sides = [[1, 0], [-1, 0], [0, 1], [0, -1]];
    for _ in 0..300 {
        let len = 2 + next(5) as i32;
        let top = range.base_sub_chunk_y
            + len
            + next((highest - range.base_sub_chunk_y - len) as u64) as i32;
        let heights: Vec<_> = (0..len).map(|offset| top - offset).collect();
        let kinds: Vec<_> = heights.iter().map(|_| next(8)).collect();
        let lights: Vec<_> = heights
            .iter()
            .map(|_| {
                (next(3) == 0).then(|| {
                    (
                        sides[next(4) as usize],
                        next(16) as u8,
                        [0, 15, next(16) as u8][next(3) as usize],
                    )
                })
            })
            .collect();
        let open_sky = next(2) == 0 && top < highest;
        let mut trusted = Vec::new();
        let mut jobs = batch_jobs(heights.iter().copied(), |stream| {
            for ((&y, &kind), light) in heights.iter().zip(&kinds).zip(&lights) {
                let key = SubChunkKey::new(0, 0, y, 0);
                match kind {
                    0 | 1 => resident(stream, key, 2),
                    2 => {
                        stream
                            .authority
                            .update_block(key, BlockUpdate::new(8, next(16) as u8, 8, 0, 1), 0)
                            .unwrap();
                        stream.sync_resident(key);
                    }
                    _ => {}
                }
                if let Some(([dx, dz], block, sky)) = *light {
                    let side = SubChunkKey::new(0, dx, y, dz);
                    install_current_light(stream, side, block, sky, false);
                    trusted.push(side);
                }
            }
            if open_sky {
                let above = SubChunkKey::new(0, 0, top + 1, 0);
                install_current_light(stream, above, 0, 15, true);
                trusted.push(above);
            }
        });
        for job in &mut jobs {
            job.prior
                .trusted_boundaries
                .extend(trusted.iter().map(|&key| (key, ())));
        }
        compare_full_column(jobs);
    }
}
