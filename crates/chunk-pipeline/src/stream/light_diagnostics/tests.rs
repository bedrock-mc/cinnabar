use super::*;

mod pipeline;

/// Creates an empty diagnostic stream without accessing a server or installed assets.
fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

#[test]
fn lighting_report_bounds_samples_columns_and_sections() {
    let mut stream = stream();
    for y in 0..MAX_SECTIONS as i32 + 10 {
        stream.diagnose_sub_chunk_reply(
            SubChunkKey::new(0, 0, y, 0),
            &PreparedSubChunkResult::AllAir,
            None,
        );
    }
    let positions = (0..20)
        .map(|x| [x as f32 * 16.0, 43.5, 0.0])
        .collect::<Vec<_>>();
    let report = stream.lighting_diagnostic(&positions);
    assert_eq!(report.matches("sample[").count(), MAX_SAMPLES);
    assert_eq!(report.matches("column=(").count(), MAX_COLUMNS);
    assert!(report.contains("samples_truncated=true"));
    assert!(report.contains("sections_truncated=true"));
    assert!(report.contains("presence=absent identity=unknown"));
    assert!(report.contains("stored_block_sky=None solved_block_sky=None solve_current=false"));
    assert!(report.contains("server_light=not-in-chunk-protocol"));
    assert!(report.len() < 30_000);
    assert_eq!(
        stream.light_diagnostics.columns[&ChunkKey::new(0, 0, 0)]
            .sections
            .len(),
        MAX_SECTIONS
    );
}

#[test]
fn lighting_report_distinguishes_stored_darkness_and_current_solve() {
    let mut stream = stream();
    let key = SubChunkKey::new(0, 0, 2, 0);
    stream.record_known_air(key);
    stream
        .lighting
        .store
        .insert_known_air(key, SubChunkLight::dark(0));
    let report = stream.lighting_diagnostic(&[[0.5, 43.5, 0.5]]);
    assert!(report.contains("presence=known-air"));
    assert!(report.contains("effective_light_filter=0 light_dampening=0"));
    assert!(
        report.contains("stored_block_sky=Some((0, 0)) solved_block_sky=None solve_current=false")
    );
    stream.lighting.block_generations.insert(key, 1);
    stream.lighting.ownership.insert(
        key,
        LightOwnership {
            block_generation: 1,
            light_revision: 0,
        },
    );
    stream.lighting.direct_sky.insert(
        key,
        StoredDirectSky {
            light_revision: 0,
            mask: Arc::new(DirectSkyMask::Uniform(false)),
        },
    );
    let report = stream.lighting_diagnostic(&[[0.5, 43.5, 0.5]]);
    assert!(report.contains("solved_block_sky=Some((0, 0)) solve_current=true"));
}

#[test]
fn lighting_provenance_retains_sources_until_eviction_and_session_modes_survive() {
    let mut stream = stream();
    let column = ChunkKey::new(0, 0, 0);
    let event = LevelChunkEvent {
        dimension: 0,
        x: 0,
        z: 0,
        mode: LevelChunkMode::Inline { count: 1 },
        payload: vec![],
    };
    stream.diagnose_inline_column(&event, &BTreeSet::new());
    let base = vanilla_dimension_range(0).unwrap().base_sub_chunk_y;
    assert_eq!(
        stream.light_diagnostics.columns[&column].sections[&base].source,
        Some("inline-empty-slot")
    );
    assert_eq!(
        stream.light_diagnostics.columns[&column].sections[&(base + 1)].source,
        Some("inline-omitted-air")
    );
    let key = SubChunkKey::from_chunk(column, base);
    let source = stream.diagnose_sub_chunk_reply(key, &PreparedSubChunkResult::AllAir, None);
    stream.diagnose_sub_chunk_commit(key, source);
    stream.diagnose_sub_chunk_reply(
        key,
        &PreparedSubChunkResult::Unavailable(
            client_world::ingestion::SubChunkUnavailable::Undefined,
        ),
        None,
    );
    assert_eq!(
        stream.light_diagnostics.columns[&column].sections[&base].source,
        Some("request-all-air")
    );
    assert_eq!(
        stream.light_diagnostics.columns[&column].sections[&base]
            .reply
            .as_deref(),
        Some("Undefined")
    );
    stream.evict_column(column);
    assert!(!stream.light_diagnostics.columns.contains_key(&column));
    assert_eq!(
        stream.lighting_request_modes()[0].as_deref(),
        Some("Inline { count: 1 }")
    );
    assert!(
        stream
            .lighting_session_facts()
            .contains("start_game_height_range=not-supplied")
    );
}

#[test]
fn report_formats_heightmaps_advertised_bounds_and_filter_above_the_eye() {
    let mut stream = stream();
    let key = SubChunkKey::new(0, 0, 10, 0);
    let decoded = world::DecodedSubChunk::decode(
        key,
        &[8, 1, 1, 2],
        &world::RawBlockIds {
            air: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        },
    );
    stream
        .authority
        .commit_decoded_sub_chunk(key, decoded)
        .unwrap();
    stream.sync_resident(key);
    let map = client_world::ingestion::HeightmapDiagnostic {
        kind: 2,
        payload_present: false,
        sample_count: 0,
        min: None,
        max: None,
    };
    stream.diagnose_sub_chunk_reply(
        key,
        &PreparedSubChunkResult::AllAir,
        Some(SubChunkDiagnostic {
            payload_present: false,
            heightmap: map,
            render_heightmap: map,
        }),
    );
    stream.apply_immediate(
        WorldEvent::DimensionHeights(vec![DimensionHeightDiagnostic {
            dimension: 0,
            minimum_y: -128,
            height_range: 512,
        }]),
        None,
    );
    let report = stream.lighting_diagnostic(&[[0.5, 43.5, 0.5]]);
    assert!(report.contains("advertised_height=Some(DimensionHeightDiagnostic { dimension: 0, minimum_y: -128, height_range: 512 })"));
    assert!(report.contains("heightmap=all-too-high(2)/present=false/samples=0/raw_min_max=None"));
    assert!(report.contains("first_filter_y=Some(175)"));
    assert!(report.contains("light_filter=15 light_dampening=15"));
    assert!(report.contains("first_unknown_y=Some("));
}

#[test]
fn failed_column_evidence_has_a_cap_and_is_pruned_without_resident_terrain() {
    let mut stream = stream();
    for x in 0..MAX_RETAINED_COLUMNS as i32 + 1 {
        stream
            .light_diagnostics
            .mode(ChunkKey::new(0, x, 0), LevelChunkMode::LimitlessRequests);
    }
    assert_eq!(stream.light_diagnostics.columns.len(), MAX_RETAINED_COLUMNS);
    assert!(stream.light_diagnostics.column_evidence_truncated);
    assert!(stream.resident.is_empty());
    assert!(stream.requests.requested.is_empty());
    stream.chunk_radius = Some(1);
    stream.reevaluate_chunk_retention();
    assert!(stream.light_diagnostics.columns.len() < MAX_RETAINED_COLUMNS);
    assert!(
        !stream
            .light_diagnostics
            .columns
            .contains_key(&ChunkKey::new(0, 100, 0))
    );
}
