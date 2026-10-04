use super::*;

#[test]
fn omitted_inline_upper_sub_chunks_carry_sky_to_open_air() {
    let mut stream = lit_stream(0);
    stream.submit(1, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    settle_light(&mut stream, [8.0, 69.0, 8.0]);
    let range = vanilla_dimension_range(0).unwrap();
    for offset in 0..range.sub_chunk_count {
        let key = SubChunkKey::new(0, 0, range.base_sub_chunk_y + offset as i32, 0);
        assert!(stream.light_is_current(key), "{key:?}");
        assert_eq!(
            stream
                .light_store
                .light(key)
                .unwrap()
                .get(LightChannel::Sky, 8, 8, 8),
            Some(15),
            "{key:?}"
        );
        assert!(stream.direct_sky[&key].mask.get(8, 8, 8), "{key:?}");
    }
}

#[test]
fn omitted_inline_upper_sub_chunks_carry_sky_above_terrain() {
    let mut stream = lit_stream(0);
    let range = vanilla_dimension_range(0).unwrap();
    let mut payload = vec![9, 1, range.base_sub_chunk_y as i8 as u8, 1, 4];
    payload.extend(biome_payload(0, 1));
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload,
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    settle_light(&mut stream, [8.0, 69.0, 8.0]);
    for offset in 1..range.sub_chunk_count {
        let key = SubChunkKey::new(0, 0, range.base_sub_chunk_y + offset as i32, 0);
        assert!(stream.light_is_current(key), "{key:?}");
        assert_eq!(
            stream
                .light_store
                .light(key)
                .unwrap()
                .get(LightChannel::Sky, 8, 8, 8),
            Some(15),
            "{key:?}"
        );
        assert!(stream.direct_sky[&key].mask.get(8, 8, 8), "{key:?}");
    }
}

#[test]
fn request_empty_upper_sub_chunks_carry_sky_to_open_air() {
    for limited in [false, true] {
        for empty in 0..3 {
            let mut stream = lit_stream(0);
            let range = vanilla_dimension_range(0).unwrap();
            let count = if limited { 1 } else { range.sub_chunk_count };
            let mode = if limited {
                LevelChunkMode::LimitedRequests {
                    highest: count as u16,
                }
            } else {
                LevelChunkMode::LimitlessRequests
            };
            stream
                .submit(1, request_level_chunk_event(0, 0, 0, mode, 1))
                .unwrap();
            complete_pending_decode_jobs(&mut stream);
            for offset in 0..count {
                let key = SubChunkKey::new(0, 0, range.base_sub_chunk_y + offset as i32, 0);
                let result = match empty {
                    0 => PreparedSubChunkResult::AllAir,
                    1 => PreparedSubChunkResult::Decoded(world::DecodedSubChunk::decode(
                        key,
                        &[],
                        &world::RawBlockIds { air: 0 },
                    )),
                    _ => {
                        PreparedSubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds)
                    }
                };
                apply_sub_chunk_result(&mut stream, key, result);
            }
            settle_light(&mut stream, [8.0, 69.0, 8.0]);
            assert_eq!(
                stream.solved_light_at([8.0, 69.0, 8.0]),
                Some((0, 15)),
                "limited={limited} empty={empty}"
            );
        }
    }
}

#[test]
fn transparent_occupied_top_cell_receives_sky_and_opaque_top_cell_filters_it() {
    for (runtime_id, expected) in [(3, 15), (2, 0)] {
        let mut stream = lit_stream(0);
        let top = SubChunkKey::new(0, 0, 19, 0);
        stream
            .authority
            .commit_sub_chunk(top, super::uniform_sub_chunk(runtime_id))
            .unwrap();
        stream.resident.insert(top);
        stream.mark_changed(top, Instant::now());
        complete_one_light(&mut stream, [8.0, 312.0, 8.0]);
        assert_eq!(
            stream
                .light_store
                .light(top)
                .unwrap()
                .get(LightChannel::Sky, 8, 15, 8),
            Some(expected)
        );
    }
}

#[test]
fn overworld_seeds_direct_sky_from_known_cells_at_dimension_top() {
    let mut stream = lit_stream(0);
    let top = SubChunkKey::new(0, 0, 19, 0);
    stream.record_known_air(top);
    stream.mark_changed(top, Instant::now());
    complete_one_light(&mut stream, [8.0, 312.0, 8.0]);

    let light = stream.light_store.light(top).unwrap();
    assert_eq!(light.get(LightChannel::Sky, 0, 15, 0), Some(15));
    assert_eq!(light.get(LightChannel::Sky, 0, 14, 0), Some(15));
    assert!(stream.direct_sky[&top].mask.get(0, 15, 0));
    assert!(stream.direct_sky[&top].mask.get(0, 14, 0));

    let below = SubChunkKey::new(0, 0, 18, 0);
    stream.record_known_air(below);
    stream.mark_changed(below, Instant::now());
    let blocks = stream.light_block_snapshot(below);
    assert_eq!(blocks.sky_seed(BlockPos::new(0, 303, 0)), 0);
    settle_light(&mut stream, [8.0, 296.0, 8.0]);
    assert_eq!(
        stream
            .light_store
            .light(below)
            .unwrap()
            .get(LightChannel::Sky, 0, 0, 0),
        Some(15)
    );
    assert!(stream.direct_sky[&below].mask.get(0, 0, 0));
    assert!(stream.light_is_current(top));
    assert!(stream.light_is_current(below));

    install_current_light(&mut stream, below, 1, 15, true);
    // A completed solve may leave an obsolete candidate in the ready heap. Keep that state
    // explicit so this test covers the empty cleanup turn regardless of worker timing.
    stream.pending_light_ready.clear();
    stream
        .pending_light_ready
        .push(PendingSchedulerCandidate::new(
            top,
            stream.light_ownership[&top].light_revision,
            SchedulerView {
                position: [8.0, 296.0, 8.0],
                forward: stream.view_forward,
            },
            false,
        ));
    stream.mark_light_dirty_exact(top);
    stream.mark_light_dirty_exact(below);
    // Nonurgent work enters the deferred heap. If ready held only obsolete revisions, one
    // turn drains those before the next promotes deferred work; both cells must still be
    // dispatched together in one batch, before any completion is accepted.
    let mut dispatched = 0;
    for _ in 0..2 {
        dispatched = stream.dispatch_light_jobs([8.0, 296.0, 8.0], 1);
        if dispatched != 0 {
            break;
        }
    }
    assert_eq!(dispatched, 2);
    assert!(stream.in_flight_light.contains_key(&top));
    assert!(stream.in_flight_light.contains_key(&below));
    assert!(stream.light_waiters.is_empty());
    for _ in 0..2 {
        let completion = stream
            .light_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        stream.accept_light_completion(completion);
    }
    assert!(stream.direct_sky[&below].mask.get(0, 0, 0));
    assert_eq!(
        stream
            .light_store
            .light(below)
            .unwrap()
            .get(LightChannel::Sky, 0, 0, 0),
        Some(15)
    );
    assert!(stream.pending_light.is_empty());
    assert!(stream.in_flight_light.is_empty());
    assert!(stream.light_is_current(top));
    assert!(stream.light_is_current(below));
}

#[test]
fn taller_columns_dispatch_the_highest_pending_source_before_its_dependency() {
    let mut stream = lit_stream(0);
    let range = vanilla_dimension_range(0).unwrap();
    let vanilla_top = range.base_sub_chunk_y + range.sub_chunk_count as i32 - 1;
    let lower = SubChunkKey::new(0, 0, vanilla_top, 0);
    let upper = SubChunkKey::new(0, 0, vanilla_top + 1, 0);
    for key in [lower, upper] {
        stream
            .authority
            .commit_sub_chunk(key, super::uniform_sub_chunk(3))
            .unwrap();
        stream.resident.insert(key);
        stream.mark_changed(key, Instant::now());
    }
    assert_eq!(
        stream.highest_pending_light_in_column(lower).unwrap().0,
        upper
    );
    assert_eq!(
        stream.light_block_snapshot(upper).overworld_top_y,
        Some(upper.y * 16 + 15)
    );
    complete_one_light(&mut stream, [8.0, upper.y as f32 * 16.0 + 8.0, 8.0]);
    assert!(stream.light_is_current(upper));
    settle_light(&mut stream, [8.0, lower.y as f32 * 16.0 + 8.0, 8.0]);
    assert!(stream.light_is_current(lower));
}
