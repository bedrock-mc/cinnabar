use super::*;

fn direct_mask_with(position: [u8; 3]) -> DirectSkyMask {
    let mut words = Box::new([0_u64; 64]);
    let index = light_local_index(position[0], position[1], position[2]);
    words[index / 64] |= 1_u64 << (index % 64);
    DirectSkyMask::Packed(words)
}

#[test]
fn dominated_lower_column_faces_do_not_redirty_current_upper_air() {
    let mut stream = lit_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let upper = SubChunkKey::new(1, 0, 1, 0);
    stream
        .store
        .commit_sub_chunk(source, super::uniform_sub_chunk(3))
        .unwrap();
    install_current_light(&mut stream, source, 0, 0, false);
    install_current_light(&mut stream, upper, 0, 14, false);
    let upper_generation = stream.light_store.light(upper).unwrap().generation();
    stream.mark_light_dirty_exact(source).unwrap();
    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1), 1);
    let mut completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("dispatched lower completion");
    let solved = completion.result.as_mut().unwrap();
    solved.replacement = SubChunkLight::uniform(0, 14, completion.identity.revision).unwrap();
    solved.direct_sky = Arc::new(DirectSkyMask::Uniform(false));
    solved.light_levels_changed = true;
    solved.direct_sky_changed = false;
    solved.changed_faces = [false, false, false, true, false, false];
    stream.accept_light_completion(completion);

    assert!(stream.light_is_current(upper));
    assert_eq!(
        stream.light_store.light(upper).unwrap().generation(),
        upper_generation,
        "dominated lower face needlessly dirtied the current upper air"
    );
    assert!(!stream.pending_light.contains_key(&upper));
    assert!(
        stream.pending_mesh.contains_key(&source),
        "light dominance must not suppress changed-face mesh invalidation"
    );
}

#[test]
fn dominated_face_does_not_redirty_current_resident_neighbour() {
    let mut stream = lit_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let neighbour = SubChunkKey::new(1, 1, 0, 0);
    for key in [source, neighbour] {
        stream
            .store
            .commit_sub_chunk(key, super::uniform_sub_chunk(3))
            .unwrap();
    }
    install_current_light(&mut stream, source, 0, 0, false);
    install_current_light(&mut stream, neighbour, 14, 0, false);
    let neighbour_generation = stream.light_store.light(neighbour).unwrap().generation();
    stream.mark_light_dirty_exact(source).unwrap();
    assert_eq!(stream.dispatch_light_jobs([8.0, 8.0, 8.0], 1), 1);
    let mut completion = stream
        .light_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("dispatched resident completion");
    let solved = completion.result.as_mut().unwrap();
    solved.replacement = SubChunkLight::uniform(15, 0, completion.identity.revision).unwrap();
    solved.direct_sky = Arc::new(DirectSkyMask::Uniform(false));
    solved.light_levels_changed = true;
    solved.direct_sky_changed = false;
    solved.changed_faces = [false, true, false, false, false, false];
    stream.accept_light_completion(completion);

    assert!(stream.light_is_current(neighbour));
    assert_eq!(
        stream.light_store.light(neighbour).unwrap().generation(),
        neighbour_generation,
        "dominated face needlessly dirtied the current resident neighbour"
    );
    assert!(!stream.pending_light.contains_key(&neighbour));
    assert!(stream.pending_mesh.contains_key(&source));
}

#[test]
fn monotonic_proof_requires_sound_prior_direct_provenance() {
    let source = SubChunkKey::new(1, 0, 0, 0);
    let mut stream = lit_stream(1);
    install_current_light(&mut stream, source, 0, 15, false);
    stream.direct_sky.remove(&source);
    assert_eq!(
        stream.monotonic_light_faces(
            source,
            &SubChunkLight::uniform(0, 15, 20_000).unwrap(),
            &DirectSkyMask::Uniform(false),
        ),
        [false; 6]
    );

    install_current_light(&mut stream, source, 0, 0, false);
    stream.direct_sky.remove(&source);
    assert_eq!(
        stream.monotonic_light_faces(
            source,
            &SubChunkLight::uniform(0, 1, 20_001).unwrap(),
            &DirectSkyMask::Uniform(false),
        ),
        [true; 6]
    );

    install_current_light(&mut stream, source, 0, 0, false);
    stream.direct_sky.get_mut(&source).unwrap().light_revision += 1;
    assert_eq!(
        stream.monotonic_light_faces(
            source,
            &SubChunkLight::uniform(0, 1, 20_002).unwrap(),
            &DirectSkyMask::Uniform(false),
        ),
        [false; 6]
    );
}

#[test]
fn monotonic_proof_checks_each_nonuniform_face_orientation() {
    let source = SubChunkKey::new(1, 0, 0, 0);
    let face_positions = [
        [0, 5, 7],
        [15, 5, 7],
        [5, 0, 7],
        [5, 15, 7],
        [5, 7, 0],
        [5, 7, 15],
    ];
    for (face, position) in face_positions.into_iter().enumerate() {
        let mut stream = lit_stream(1);
        install_current_light(&mut stream, source, 0, 0, false);
        let generation = stream.light_store.light(source).unwrap().generation();
        let mut previous = SubChunkLight::dark(generation);
        previous
            .set(
                LightChannel::Block,
                position[0],
                position[1],
                position[2],
                1,
            )
            .unwrap();
        stream.light_store.insert_known_air(source, previous);
        let replacement = SubChunkLight::dark(generation + 1);
        let flags =
            stream.monotonic_light_faces(source, &replacement, &DirectSkyMask::Uniform(false));
        let mut expected = [true; 6];
        expected[face] = false;
        assert_eq!(flags, expected, "single-cell decrease on face {face}");

        let mut previous = SubChunkLight::dark(generation);
        previous
            .set(LightChannel::Sky, position[0], position[1], position[2], 15)
            .unwrap();
        stream.light_store.insert_known_air(source, previous);
        stream.direct_sky.get_mut(&source).unwrap().mask = Arc::new(direct_mask_with(position));
        let mut replacement = SubChunkLight::dark(generation + 1);
        replacement
            .set(LightChannel::Sky, position[0], position[1], position[2], 15)
            .unwrap();
        let flags =
            stream.monotonic_light_faces(source, &replacement, &DirectSkyMask::Uniform(false));
        assert_eq!(flags, expected, "single-cell direct loss on face {face}");
    }
}

#[test]
fn dominance_proof_maps_each_nonuniform_source_cell_to_the_opposite_face() {
    let source = SubChunkKey::new(1, 0, 0, 0);
    let probes = [
        ([0, 5, 7], [15, 5, 7]),
        ([15, 5, 7], [0, 5, 7]),
        ([5, 0, 7], [5, 15, 7]),
        ([5, 15, 7], [5, 0, 7]),
        ([5, 7, 0], [5, 7, 15]),
        ([5, 7, 15], [5, 7, 0]),
    ];
    for (face, (source_position, destination_position)) in probes.into_iter().enumerate() {
        let destination = offset_sub_chunk_key(source, LIGHT_NEIGHBOUR_OFFSETS[face]).unwrap();
        let mut stream = lit_stream(1);
        install_current_light(&mut stream, source, 0, 0, false);
        install_current_light(&mut stream, destination, 0, 0, false);
        let source_generation = stream.light_store.light(source).unwrap().generation();
        let destination_generation = stream.light_store.light(destination).unwrap().generation();

        let mut replacement = SubChunkLight::dark(source_generation);
        replacement
            .set(
                LightChannel::Block,
                source_position[0],
                source_position[1],
                source_position[2],
                15,
            )
            .unwrap();
        let monotonic_faces =
            stream.monotonic_light_faces(source, &replacement, &DirectSkyMask::Uniform(false));
        stream.light_store.insert_known_air(source, replacement);

        let mut dominated = SubChunkLight::dark(destination_generation);
        dominated
            .set(
                LightChannel::Block,
                destination_position[0],
                destination_position[1],
                destination_position[2],
                14,
            )
            .unwrap();
        stream.light_store.insert_known_air(destination, dominated);
        assert!(stream.current_known_target_dominates_source_face(
            source,
            destination,
            monotonic_faces
        ));

        let mut exceeded = SubChunkLight::dark(destination_generation);
        exceeded
            .set(
                LightChannel::Block,
                destination_position[0],
                destination_position[1],
                destination_position[2],
                13,
            )
            .unwrap();
        stream.light_store.insert_known_air(destination, exceeded);
        assert!(!stream.current_known_target_dominates_source_face(
            source,
            destination,
            monotonic_faces
        ));
    }
}

#[test]
fn dominance_proof_fails_closed_for_decrease_provenance_and_target_state() {
    let above = SubChunkKey::new(1, 0, 1, 0);
    let below = SubChunkKey::new(1, 0, 0, 0);
    let mut stream = lit_stream(1);
    install_current_light(&mut stream, above, 0, 15, false);
    install_current_light(&mut stream, below, 0, 15, false);
    let gained_direct = DirectSkyMask::Uniform(true);
    let gained_faces = stream.monotonic_light_faces(
        above,
        &SubChunkLight::uniform(0, 15, 30_000).unwrap(),
        &gained_direct,
    );
    stream.direct_sky.get_mut(&above).unwrap().mask = Arc::new(gained_direct);
    assert!(!stream.current_known_target_dominates_source_face(above, below, gained_faces));
    install_current_light(&mut stream, below, 0, 15, true);
    assert!(stream.current_known_target_dominates_source_face(above, below, gained_faces));

    install_current_light(&mut stream, above, 15, 15, true);
    let removal_faces = stream.monotonic_light_faces(
        above,
        &SubChunkLight::dark(30_001),
        &DirectSkyMask::Uniform(false),
    );
    assert_eq!(removal_faces, [false; 6]);

    install_current_light(&mut stream, above, 0, 15, true);
    let lost_direct = stream.monotonic_light_faces(
        above,
        &SubChunkLight::uniform(0, 15, 30_002).unwrap(),
        &DirectSkyMask::Uniform(false),
    );
    assert!(!lost_direct[2]);

    let side = SubChunkKey::new(1, 1, 1, 0);
    install_current_light(&mut stream, side, 0, 14, false);
    let dominated_faces = stream.monotonic_light_faces(
        above,
        &SubChunkLight::uniform(0, 15, 30_003).unwrap(),
        &DirectSkyMask::Uniform(true),
    );
    stream.direct_sky.get_mut(&side).unwrap().light_revision += 1;
    assert!(!stream.current_known_target_dominates_source_face(above, side, dominated_faces));
    install_current_light(&mut stream, side, 0, 14, false);
    *stream.block_generations.get_mut(&side).unwrap() += 1;
    assert!(!stream.current_known_target_dominates_source_face(above, side, dominated_faces));
    install_current_light(&mut stream, side, 0, 14, false);
    stream.mark_light_dirty_exact(side).unwrap();
    assert!(!stream.current_known_target_dominates_source_face(above, side, dominated_faces));
    stream.pending_light.remove(&side);
    let identity = LightJobIdentity {
        revision: stream.light_revisions.dirty(side).unwrap().revision,
        block_generation: stream.block_generations[&side],
        previous_light_generation: Some(stream.light_store.light(side).unwrap().generation()),
        batch_id: 90_000,
        urgent: false,
    };
    stream.in_flight_light.insert(side, identity);
    assert!(!stream.current_known_target_dominates_source_face(above, side, dominated_faces));
    stream.in_flight_light.remove(&side);
    stream.light_revisions.entries.remove(&side);

    let non_air = SubChunkKey::new(1, -1, 1, 0);
    stream
        .store
        .commit_sub_chunk(non_air, super::uniform_sub_chunk(3))
        .unwrap();
    install_current_light(&mut stream, non_air, 0, 14, false);
    assert!(stream.current_known_target_dominates_source_face(above, non_air, dominated_faces));
    let source_direct = stream.direct_sky.remove(&above).unwrap();
    assert!(!stream.current_known_target_dominates_source_face(above, non_air, dominated_faces));
    stream.direct_sky.insert(above, source_direct);

    stream
        .store
        .commit_sub_chunk(below, super::uniform_sub_chunk(3))
        .unwrap();
    install_current_light(&mut stream, below, 0, 15, false);
    assert!(!stream.current_known_target_dominates_source_face(above, below, gained_faces));
    install_current_light(&mut stream, below, 0, 15, true);
    assert!(stream.current_known_target_dominates_source_face(above, below, gained_faces));

    let unknown = SubChunkKey::new(1, 0, 1, 1);
    assert!(!stream.current_known_target_dominates_source_face(above, unknown, dominated_faces));
}

#[test]
fn resident_dominance_uses_fast_unit_bound_then_exact_destination_filter() {
    let source = SubChunkKey::new(1, 0, 0, 0);
    for runtime_id in [2, 99_999] {
        let destination = SubChunkKey::new(1, 1, 0, 0);
        let mut stream = lit_stream(1);
        install_current_light(&mut stream, source, 0, 0, false);
        let source_generation = stream.light_store.light(source).unwrap().generation();
        let replacement = SubChunkLight::uniform(15, 0, source_generation).unwrap();
        let monotonic_faces =
            stream.monotonic_light_faces(source, &replacement, &DirectSkyMask::Uniform(false));
        stream.light_store.insert_known_air(source, replacement);
        stream
            .store
            .commit_sub_chunk(destination, super::uniform_sub_chunk(runtime_id))
            .unwrap();
        install_current_light(&mut stream, destination, 14, 0, false);

        assert!(stream.current_known_target_dominates_source_face(
            source,
            destination,
            monotonic_faces
        ));

        install_current_light(&mut stream, destination, 13, 0, false);
        assert!(stream.current_known_target_dominates_source_face(
            source,
            destination,
            monotonic_faces
        ));
    }

    let destination = SubChunkKey::new(1, 1, 0, 0);
    let mut stream = lit_stream(1);
    install_current_light(&mut stream, source, 0, 0, false);
    let source_generation = stream.light_store.light(source).unwrap().generation();
    let replacement = SubChunkLight::uniform(15, 0, source_generation).unwrap();
    let monotonic_faces =
        stream.monotonic_light_faces(source, &replacement, &DirectSkyMask::Uniform(false));
    stream.light_store.insert_known_air(source, replacement);
    stream
        .store
        .commit_sub_chunk(destination, super::uniform_sub_chunk(3))
        .unwrap();
    install_current_light(&mut stream, destination, 13, 0, false);
    assert!(!stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));
}

#[test]
fn waiter_requeues_only_when_new_contribution_exceeds_current_air() {
    fn accept_waiter_change(destination_level: u8) -> WorldStream {
        let mut stream = lit_stream(1);
        let source = SubChunkKey::new(1, 0, 0, 0);
        let destination = SubChunkKey::new(1, 1, 0, 0);
        install_current_light(&mut stream, source, 0, 0, false);
        install_current_light(&mut stream, destination, destination_level, 0, false);
        stream
            .light_waiters
            .entry(source)
            .or_default()
            .insert(destination);
        let mut completion = synthetic_light_completion(
            &mut stream,
            source,
            DirectSkyMask::Uniform(false),
            true,
            false,
            [false; 6],
        );
        completion.result.as_mut().unwrap().replacement =
            SubChunkLight::uniform(15, 0, completion.identity.revision).unwrap();
        stream.accept_light_completion(completion);
        stream
    }

    let dominated = accept_waiter_change(14);
    let destination = SubChunkKey::new(1, 1, 0, 0);
    assert!(dominated.light_is_current(destination));
    assert!(!dominated.pending_light.contains_key(&destination));

    let exceeded = accept_waiter_change(0);
    assert!(!exceeded.light_is_current(destination));
    assert!(exceeded.pending_light.contains_key(&destination));
}

#[test]
#[ignore = "release-only dense boundary-dominance comparison"]
fn release_dense_mixed_boundary_dominance_benchmark() {
    fn drain(stream: &mut WorldStream) -> (Duration, [u64; 5]) {
        let before = stream.stats();
        let started = Instant::now();
        let mut stalled = 0;
        while !stream.pending_light.is_empty() || !stream.in_flight_light.is_empty() {
            stream.dispatch_light_jobs([8.0, 80.0, 8.0], usize::MAX);
            if stream.in_flight_light.is_empty() {
                stalled += 1;
                assert!(stalled <= 512, "dense boundary benchmark stalled");
                continue;
            }
            stalled = 0;
            let completion = stream
                .light_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("dense boundary benchmark completion");
            stream.accept_light_completion(completion);
            while let Ok(completion) = stream.light_rx.try_recv() {
                stream.accept_light_completion(completion);
            }
        }
        let after = stream.stats();
        (
            started.elapsed(),
            [
                after.accepted_light_jobs - before.accepted_light_jobs,
                after.value_changed_light_jobs - before.value_changed_light_jobs,
                after.noop_light_jobs - before.noop_light_jobs,
                after.provenance_only_light_jobs - before.provenance_only_light_jobs,
                after.stale_light_jobs - before.stale_light_jobs,
            ],
        )
    }

    fn exact_light_hash(stream: &WorldStream, keys: &[SubChunkKey]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for key in keys {
            let light = stream.light_store.light(*key).unwrap();
            for x in 0_u8..16 {
                for z in 0_u8..16 {
                    for y in 0_u8..16 {
                        for value in [
                            light.get(LightChannel::Block, x, y, z).unwrap(),
                            light.get(LightChannel::Sky, x, y, z).unwrap(),
                            u8::from(stream.direct_sky[key].mask.get(x, y, z)),
                        ] {
                            hash ^= u64::from(value);
                            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                        }
                    }
                }
            }
        }
        hash
    }

    fn assert_settled(stream: &WorldStream, keys: &[SubChunkKey]) {
        assert!(keys.iter().all(|key| stream.light_is_current(*key)));
        assert!(stream.pending_light.is_empty());
        assert!(stream.in_flight_light.is_empty());
        assert!(stream.light_waiters.is_empty());
    }

    let mut stream = lit_stream(0);
    let mut keys = Vec::new();
    for x in -1..=1 {
        for z in -1..=1 {
            for y in -4..20 {
                let key = SubChunkKey::new(0, x, y, z);
                let runtime_id = if y == 19 && (x + z) & 1 == 0 {
                    Some(2)
                } else if y == 0 && x == 0 && z == 0 {
                    Some(1)
                } else if y == 8 && (x - z).abs() == 1 {
                    Some(3)
                } else {
                    None
                };
                if let Some(runtime_id) = runtime_id {
                    stream
                        .store
                        .commit_sub_chunk(key, super::uniform_sub_chunk(runtime_id))
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
    let initial = drain(&mut stream);
    assert_settled(&stream, &keys);
    let initial_hash = exact_light_hash(&stream, &keys);
    assert_eq!(initial_hash, 0x8406_a83a_02b2_6fbd);

    let emitter = SubChunkKey::new(0, 0, 0, 0);
    stream
        .store
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(0))
        .unwrap();
    stream.mark_changed(emitter, Instant::now());
    let removal = drain(&mut stream);
    assert_settled(&stream, &keys);
    let removal_hash = exact_light_hash(&stream, &keys);
    assert_eq!(removal_hash, 0xfdb3_31a6_0caa_17e5);

    stream
        .store
        .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
        .unwrap();
    stream.mark_changed(emitter, Instant::now());
    let addition = drain(&mut stream);
    assert_settled(&stream, &keys);
    let addition_hash = exact_light_hash(&stream, &keys);
    assert_eq!(addition_hash, 0x8406_a83a_02b2_6fbd);
    eprintln!(
        "dense mixed boundary benchmark: initial={initial:?} removal={removal:?} \
         addition={addition:?} hashes={initial_hash:016x}/{removal_hash:016x}/{addition_hash:016x}"
    );
}
