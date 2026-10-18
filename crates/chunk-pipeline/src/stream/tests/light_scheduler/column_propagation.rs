use super::*;

#[test]
fn original_column_lighting_waits_for_the_loaded_three_by_three_context() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 4, 0, 6);
    stream.record_known_air(key);
    for dx in -1..=1 {
        for dz in -1..=1 {
            stream
                .publisher
                .required_columns
                .insert(ChunkKey::new(1, key.x + dx, key.z + dz));
        }
    }
    stream.loaded_columns.insert(key.chunk());
    stream.mark_changed(key, Instant::now());

    assert_eq!(stream.dispatch_light_jobs([72.0, 8.0, 104.0], 1), 0);
    for dx in -1..=1 {
        for dz in -1..=1 {
            stream
                .loaded_columns
                .insert(ChunkKey::new(1, key.x + dx, key.z + dz));
        }
    }

    assert_eq!(stream.dispatch_light_jobs([72.0, 8.0, 104.0], 1), 1);
}

#[test]
fn original_column_lighting_treats_unrequested_neighbours_as_the_view_boundary() {
    let mut stream = lit_stream(1);
    let key = SubChunkKey::new(1, 4, 0, 6);
    stream.record_known_air(key);
    stream.publisher.required_columns.insert(key.chunk());
    stream.loaded_columns.insert(key.chunk());
    stream.mark_changed(key, Instant::now());

    assert_eq!(stream.dispatch_light_jobs([72.0, 8.0, 104.0], 1), 1);
}

#[test]
fn limited_empty_column_propagates_direct_sky_into_the_mesh_light_sidecar() {
    let mut stream = lit_stream(0);
    stream
        .submit(
            1,
            super::request_level_chunk_event(
                0,
                0,
                0,
                protocol::LevelChunkMode::LimitedRequests { highest: 0 },
                1,
            ),
        )
        .unwrap();
    super::complete_pending_decode_jobs(&mut stream);

    assert!(stream.take_requests().is_empty());
    settle_light(&mut stream, [8.0, 312.0, 8.0]);

    let bottom = SubChunkKey::new(0, 0, -4, 0);
    assert_eq!(
        stream
            .lighting
            .store
            .light(bottom)
            .unwrap()
            .get(LightChannel::Sky, 0, 0, 0),
        Some(15)
    );
    assert!(stream.lighting.direct_sky[&bottom].mask.get(0, 0, 0));
    let halo = stream
        .mesh_light_halo(bottom)
        .expect("settled implicit air exposes a current mesh-light sidecar");
    assert_eq!(halo.sample_channels([0, 0, 0]), [0, 15]);
}

#[test]
fn nether_and_end_never_seed_sky() {
    for (dimension, y) in [(1, 7), (2, 15)] {
        let mut stream = lit_stream(dimension);
        let key = SubChunkKey::new(dimension, 0, y, 0);
        stream.record_known_air(key);
        stream.mark_changed(key, Instant::now());
        complete_one_light(&mut stream, [8.0, y as f32 * 16.0 + 8.0, 8.0]);

        assert_eq!(
            stream
                .lighting
                .store
                .light(key)
                .unwrap()
                .get(LightChannel::Sky, 0, 15, 0),
            Some(0)
        );
        assert!(!stream.lighting.direct_sky[&key].mask.get(0, 15, 0));
    }
}

#[test]
fn overworld_initial_sky_work_waits_for_the_current_upper_subchunk() {
    let mut stream = lit_stream(0);
    let top = SubChunkKey::new(0, 0, 19, 0);
    let below = SubChunkKey::new(0, 0, 18, 0);
    stream.record_known_air(top);
    stream.record_known_air(below);
    stream.mark_light_changed_sources([top, below]);

    assert_eq!(stream.dispatch_light_jobs([8.0, 296.0, 8.0], 4), 2);
    assert!(stream.lighting.jobs.in_flight.contains_key(&top));
    assert!(stream.lighting.jobs.in_flight.contains_key(&below));
}

#[test]
fn mixed_vertical_batches_propagate_emission_into_air_above() {
    for (dimension, base_y, expected_sky) in [(1, 0, 0), (0, 17, 15)] {
        let mut stream = lit_stream(dimension);
        let emitter = SubChunkKey::new(dimension, 0, base_y, 0);
        let middle = SubChunkKey::new(dimension, 0, base_y + 1, 0);
        let top = SubChunkKey::new(dimension, 0, base_y + 2, 0);
        stream
            .authority
            .commit_sub_chunk(emitter, super::uniform_sub_chunk(1))
            .unwrap();
        stream.resident.insert(emitter);
        stream.record_known_air(middle);
        stream.record_known_air(top);
        stream.mark_light_changed_sources([emitter, middle, top]);
        settle_light(&mut stream, [8.0, (base_y * 16 + 40) as f32, 8.0]);
        for key in [emitter, middle, top] {
            assert!(stream.light_is_current(key));
        }
        let light = stream.lighting.store.light(middle).unwrap();
        assert_eq!(
            light.get(LightChannel::Block, 0, 0, 0),
            Some(14),
            "dimension {dimension}"
        );
        assert_eq!(
            light.get(LightChannel::Sky, 0, 0, 0),
            Some(expected_sky),
            "dimension {dimension}"
        );
        assert!(stream.lighting.waiters.is_empty());
    }
}
