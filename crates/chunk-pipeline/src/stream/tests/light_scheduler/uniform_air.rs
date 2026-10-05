use super::*;
use std::hint::black_box;

fn air_job(dimension: i32, sky: u8, direct: bool) -> PreparedLightJob {
    let mut stream = lit_stream(dimension);
    let key = SubChunkKey::new(dimension, 0, 18, 0);
    for neighbour in key.mesh_dependents() {
        install_current_light(&mut stream, neighbour, 0, sky, direct);
    }
    let revision = stream.mark_light_dirty_exact(key).unwrap();
    PreparedLightJob {
        key,
        identity: LightJobIdentity {
            revision,
            block_generation: stream.lighting.block_generations[&key],
            previous_light_generation: stream
                .lighting
                .store
                .light(key)
                .map(|light| light.generation()),
            batch_id: 1,
            urgent: false,
        },
        blocks: stream.light_block_snapshot(key),
        prior: stream.light_prior_snapshot(key),
        bounds: light_bounds(key).unwrap(),
        queued_at: Instant::now(),
    }
}

fn assert_matches_oracles(mut job: PreparedLightJob) {
    let actual = uniform_known_air_light(&job);
    let scanned = scanned_uniform_known_air_light(&job);
    assert_eq!(actual.is_some(), scanned.is_some());
    let Some((light, direct)) = actual else {
        return;
    };
    job.blocks.resolve_palette_light();
    let full = solve_light(
        &job.blocks,
        &job.prior,
        job.bounds,
        job.identity.revision,
        job.blocks.profile,
        LIGHT_SOLVE_LIMITS,
    )
    .unwrap();
    let solved = full.sub_chunks()[&job.key].as_ref();
    assert!(light_levels_equal(&light, solved));
    assert_eq!(direct, DirectSkyMask::from_output(&full, job.key));
}

#[test]
fn uniform_air_channels_match_full_solver_and_scanned_boundaries() {
    for (dimension, sky, direct) in [(0, 15, true), (0, 0, false), (1, 0, false), (2, 0, false)] {
        let job = air_job(dimension, sky, direct);
        for neighbour in job.key.mesh_dependents().filter(|key| *key != job.key) {
            assert!(uniform_boundary_sample(&job.prior, neighbour, LightChannel::Block).is_some());
            assert!(uniform_boundary_sample(&job.prior, neighbour, LightChannel::Sky).is_some());
        }
        assert_matches_oracles(job);
    }
    let mut top = air_job(0, 0, false);
    top.blocks.overworld_top_y = Some(top.bounds.max().y);
    assert_matches_oracles(top);
}

#[test]
fn packed_light_and_direct_provenance_keep_exact_face_fallbacks() {
    for offset in LIGHT_NEIGHBOUR_OFFSETS {
        let mut job = air_job(1, 0, false);
        let neighbour = offset_sub_chunk_key(job.key, offset).unwrap();
        let mut light = job.prior.light.light(neighbour).unwrap().as_ref().clone();
        light.set(LightChannel::Block, 8, 8, 8, 9).unwrap();
        job.prior
            .light
            .replace_known_light(neighbour, Arc::new(light));
        assert!(uniform_boundary_sample(&job.prior, neighbour, LightChannel::Block).is_none());
        assert!(uniform_known_air_light(&job).is_some());
        assert_matches_oracles(job);

        let mut job = air_job(1, 0, false);
        let neighbour = offset_sub_chunk_key(job.key, offset).unwrap();
        let position = light_boundary_position(job.key, offset, 5, 7).unwrap();
        let (_, [x, y, z]) = split_light_position(job.key.dimension, position);
        let mut light = job.prior.light.light(neighbour).unwrap().as_ref().clone();
        light.set(LightChannel::Block, x, y, z, 9).unwrap();
        job.prior
            .light
            .replace_known_light(neighbour, Arc::new(light));
        assert!(uniform_known_air_light(&job).is_none());
        assert_matches_oracles(job);
    }
    let mut job = air_job(0, 15, true);
    let above = offset_sub_chunk_key(job.key, [0, 1, 0]).unwrap();
    let mut words = Box::new([u64::MAX; 64]);
    let index = light_local_index(8, 8, 8);
    words[index / 64] &= !(1 << (index % 64));
    job.prior.direct_sky.get_mut(&above).unwrap().mask = Arc::new(DirectSkyMask::Packed(words));
    assert!(uniform_boundary_sample(&job.prior, above, LightChannel::Sky).is_none());
    assert_matches_oracles(job);
}

#[test]
fn boundary_uniform_proofs_preserve_trust_and_provenance_generation() {
    let mut job = air_job(0, 15, true);
    let above = offset_sub_chunk_key(job.key, [0, 1, 0]).unwrap();
    job.prior.trusted_boundaries.remove(&above);
    assert_eq!(
        uniform_boundary_sample(&job.prior, above, LightChannel::Sky),
        Some(BoundaryLightSample::untrusted())
    );
    assert_matches_oracles(job);
    let mut job = air_job(0, 15, true);
    job.prior.direct_sky.get_mut(&above).unwrap().light_revision += 1;
    assert_eq!(
        uniform_boundary_sample(&job.prior, above, LightChannel::Sky),
        Some(BoundaryLightSample::trusted(15, false).unwrap())
    );
    assert_matches_oracles(job);
}

#[test]
fn uniform_air_boundary_timing() {
    if std::env::var_os("CINNABAR_JOIN_CACHE_BENCH").is_none() {
        eprintln!("missing fixture: CINNABAR_JOIN_CACHE_BENCH=1 for uniform-air boundary timing");
        return;
    }
    let job = air_job(0, 15, true);
    const ITERATIONS: usize = 1024;
    let measure = |optimized: bool| {
        let started = Instant::now();
        for _ in 0..ITERATIONS {
            black_box(if optimized {
                uniform_known_air_light(black_box(&job))
            } else {
                scanned_uniform_known_air_light(black_box(&job))
            });
        }
        started.elapsed().as_nanos() as f64 / ITERATIONS as f64
    };
    let mut before = Vec::new();
    let mut after = Vec::new();
    for _ in 0..11 {
        before.push(measure(false));
        after.push(measure(true));
    }
    before.sort_by(f64::total_cmp);
    after.sort_by(f64::total_cmp);
    eprintln!(
        "uniform_air_boundary scanned_p50_ns={:.1} uniform_p50_ns={:.1} scanned_boundary_cells=1792 uniform_boundary_cells=0",
        before[5], after[5]
    );
}

fn scanned_uniform_known_air_light(
    job: &PreparedLightJob,
) -> Option<(SubChunkLight, DirectSkyMask)> {
    if !matches!(
        job.blocks.blocks.get(&job.key),
        Some(SnapshotBlock::KnownAir)
    ) {
        return None;
    }
    let trusted_zero = BoundaryLightSample::trusted(0, false).ok()?;
    for offset in LIGHT_NEIGHBOUR_OFFSETS {
        let neighbour = offset_sub_chunk_key(job.key, offset)?;
        if !job.prior.trusted_boundaries.contains(&neighbour) {
            continue;
        }
        for a in 0_u8..16 {
            for b in 0_u8..16 {
                let position = light_boundary_position(job.key, offset, a, b)?;
                let sample =
                    job.prior
                        .boundary_light(job.key.dimension, position, LightChannel::Block);
                if sample != BoundaryLightSample::unknown()
                    && sample != BoundaryLightSample::untrusted()
                    && sample != trusted_zero
                {
                    return None;
                }
            }
        }
    }

    let (sky, direct) = match job.blocks.profile {
        DimensionLightProfile::Nether | DimensionLightProfile::End => (0, false),
        DimensionLightProfile::Overworld { .. }
            if job.blocks.overworld_top_y == job.key.y.checked_mul(16)?.checked_add(15) =>
        {
            (15, true)
        }
        DimensionLightProfile::Overworld { .. } => {
            let direct_above = (0_u8..16).all(|x| {
                (0_u8..16).all(|z| {
                    let Some(position) = light_boundary_position(job.key, [0, 1, 0], x, z) else {
                        return false;
                    };
                    job.prior
                        .boundary_light(job.key.dimension, position, LightChannel::Sky)
                        == BoundaryLightSample::trusted(15, true)
                            .expect("constant sky nibble is valid")
                })
            });
            if direct_above {
                (15, true)
            } else {
                for offset in LIGHT_NEIGHBOUR_OFFSETS {
                    let neighbour = offset_sub_chunk_key(job.key, offset)?;
                    if !job.prior.trusted_boundaries.contains(&neighbour) {
                        continue;
                    }
                    for a in 0_u8..16 {
                        for b in 0_u8..16 {
                            let position = light_boundary_position(job.key, offset, a, b)?;
                            let sample = job.prior.boundary_light(
                                job.key.dimension,
                                position,
                                LightChannel::Sky,
                            );
                            if sample != BoundaryLightSample::unknown()
                                && sample != BoundaryLightSample::untrusted()
                                && sample != trusted_zero
                            {
                                return None;
                            }
                        }
                    }
                }
                (0, false)
            }
        }
    };
    Some((
        SubChunkLight::uniform(0, sky, job.identity.revision)
            .expect("constant light nibbles are valid"),
        DirectSkyMask::Uniform(direct),
    ))
}
