use super::*;

#[test]
fn adjacent_light_jobs_converge_from_either_camera_priority() {
    for camera in [[0.0, 8.0, 8.0], [32.0, 8.0, 8.0]] {
        let mut stream = lit_stream(1);
        let emitter = SubChunkKey::new(1, 0, 0, 0);
        let air = SubChunkKey::new(1, 1, 0, 0);
        stream
            .authority
            .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
            .unwrap();
        stream.resident.insert(emitter);
        stream.record_known_air(air);
        stream.mark_light_changed_sources([emitter, air]);
        assert_eq!(stream.dispatch_light_jobs(camera, 2), 1);
        settle_light(&mut stream, camera);

        assert_eq!(
            stream
                .lighting
                .store
                .light(air)
                .unwrap()
                .get(LightChannel::Block, 0, 0, 0),
            Some(14)
        );
        assert!(stream.light_is_current(emitter));
        assert!(stream.light_is_current(air));
        assert!(stream.lighting.waiters.is_empty());
    }
}

#[test]
fn mutually_dependent_lit_air_regions_converge() {
    let mut stream = lit_stream(1);
    let emitter = SubChunkKey::new(1, 0, 0, 0);
    let air = [
        SubChunkKey::new(1, 1, 0, 0),
        SubChunkKey::new(1, 0, 0, 1),
        SubChunkKey::new(1, 1, 0, 1),
    ];
    stream
        .authority
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(emitter);
    for key in air {
        stream.record_known_air(key);
    }
    stream.mark_light_changed_sources(std::iter::once(emitter).chain(air));
    let mut completions = 0;
    while !stream.lighting.jobs.pending.is_empty() || !stream.lighting.jobs.in_flight.is_empty() {
        stream.dispatch_light_jobs([24.0, 8.0, 24.0], usize::MAX);
        let completion = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(2))
            .expect("light dependencies must make progress");
        stream.accept_light_completion(completion);
        completions += 1;
        assert!(
            completions <= 128,
            "stationary light dependencies did not converge"
        );
    }
    for key in std::iter::once(emitter).chain(air) {
        assert!(stream.light_is_current(key));
    }
    assert!(stream.lighting.waiters.is_empty());
}

#[test]
fn staggered_emitter_region_converges_to_exact_light() {
    let mut stream = lit_stream(1);
    let mut keys = Vec::new();
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            let key = SubChunkKey::new(1, chunk_x, 0, chunk_z);
            if chunk_x == 0 && chunk_z == 0 {
                stream
                    .authority
                    .commit_sub_chunk(key, super::uniform_sub_chunk(1))
                    .unwrap();
                stream.resident.insert(key);
            } else {
                stream.record_known_air(key);
            }
            keys.push(key);
        }
    }
    stream.mark_light_changed_sources(keys.iter().copied());

    let mut completions = 0_usize;
    while !stream.lighting.jobs.pending.is_empty() || !stream.lighting.jobs.in_flight.is_empty() {
        stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1);
        let completion = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(2))
            .expect("staggered light region must make progress");
        stream.accept_light_completion(completion);
        completions += 1;
        assert!(
            completions <= 128,
            "staggered light region did not converge"
        );
    }

    assert_eq!(stream.stats().stale_light_jobs, 0);
    assert!(stream.lighting.jobs.pending.is_empty());
    assert!(stream.lighting.jobs.in_flight.is_empty());
    assert!(stream.lighting.waiters.is_empty());
    for key in keys {
        assert!(stream.light_is_current(key));
        for x in 0_u8..16 {
            for z in 0_u8..16 {
                for y in 0_u8..16 {
                    let world_x = key.x * 16 + i32::from(x);
                    let world_z = key.z * 16 + i32::from(z);
                    let dx = if world_x < 0 {
                        -world_x
                    } else if world_x > 15 {
                        world_x - 15
                    } else {
                        0
                    };
                    let dz = if world_z < 0 {
                        -world_z
                    } else if world_z > 15 {
                        world_z - 15
                    } else {
                        0
                    };
                    let expected = u8::try_from(15_i32.saturating_sub(dx + dz).max(0)).unwrap();
                    let position = BlockPos::new(world_x, i32::from(y), world_z);
                    assert_eq!(
                        stream
                            .lighting
                            .store
                            .light(key)
                            .unwrap()
                            .get(LightChannel::Block, x, y, z),
                        Some(expected),
                        "wrong block light at {position:?}"
                    );
                    assert_eq!(
                        stream
                            .lighting
                            .store
                            .light(key)
                            .unwrap()
                            .get(LightChannel::Sky, x, y, z),
                        Some(0),
                        "wrong sky light at {position:?}"
                    );
                    assert!(
                        !stream
                            .lighting
                            .direct_sky
                            .get(&key)
                            .unwrap()
                            .mask
                            .get(x, y, z)
                    );
                }
            }
        }
    }
}

#[test]
fn mixed_roof_and_air_columns_converge() {
    for roof_mask in [1_u8, 3, 5, 7] {
        let mut stream = lit_stream(0);
        let mut keys = Vec::new();
        for x in 0..2 {
            for z in 0..2 {
                for y in 17..=19 {
                    let key = SubChunkKey::new(0, x, y, z);
                    let roof = y == 19 && roof_mask & (1 << (x * 2 + z)) != 0;
                    if roof || (y == 18 && x == z) {
                        stream
                            .authority
                            .commit_sub_chunk(
                                key,
                                super::uniform_sub_chunk(if roof { 2 } else { 3 }),
                            )
                            .unwrap();
                        stream.resident.insert(key);
                    } else {
                        stream.record_known_air(key);
                    }
                    keys.push(key);
                }
            }
        }
        stream.mark_light_changed_sources(keys.iter().copied());
        let mut completions = 0;
        while !stream.lighting.jobs.pending.is_empty() || !stream.lighting.jobs.in_flight.is_empty()
        {
            stream.dispatch_light_jobs([24.0, 280.0, 24.0], usize::MAX);
            let completion = stream
                .lighting
                .rx
                .recv_timeout(Duration::from_secs(2))
                .expect("roof light dependencies must make progress");
            stream.accept_light_completion(completion);
            completions += 1;
            assert!(completions <= 512, "roof mask {roof_mask} did not converge");
        }
        for key in keys {
            assert!(stream.light_is_current(key));
        }
    }
}

#[test]
fn adjacent_initial_emitter_and_air_converge_without_stale_completions() {
    let mut stream = lit_stream(1);
    let emitter = SubChunkKey::new(1, 0, 0, 0);
    let air = SubChunkKey::new(1, 1, 0, 0);
    stream
        .authority
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.resident.insert(emitter);
    stream.record_known_air(air);
    stream.mark_light_changed_sources([emitter, air]);

    let mut completions = 0_usize;
    while !stream.lighting.jobs.pending.is_empty() || !stream.lighting.jobs.in_flight.is_empty() {
        stream.dispatch_light_jobs([16.0, 8.0, 8.0], usize::MAX);
        let completion = stream
            .lighting
            .rx
            .recv_timeout(Duration::from_secs(2))
            .expect("adjacent initial light convergence");
        stream.accept_light_completion(completion);
        completions += 1;
        assert!(completions <= 3, "adjacent light convergence churned");
    }

    assert_eq!(stream.stats().stale_light_jobs, 0);
    assert_eq!(
        stream
            .lighting
            .store
            .light(air)
            .unwrap()
            .get(LightChannel::Block, 0, 0, 0),
        Some(14)
    );
}
