use super::*;

fn filtered_stream(dimension: i32) -> WorldStream {
    WorldStream::new_with_assets(
        WorldBootstrap {
            dimension,
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(filtered_light_test_assets()),
        [0.0, 80.0, 0.0],
        None,
    )
}

fn filtered_light_test_assets() -> RuntimeAssets {
    let visuals = [
        (BlockFlags::AIR, VisualKind::Invisible, ContributorRole::Air),
        (
            BlockFlags::CUBE_GEOMETRY,
            VisualKind::Cube,
            ContributorRole::Primary,
        ),
        (
            BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE,
            VisualKind::Cube,
            ContributorRole::Primary,
        ),
        (
            BlockFlags::CUBE_GEOMETRY,
            VisualKind::Cube,
            ContributorRole::Primary,
        ),
        (
            BlockFlags::CUBE_GEOMETRY,
            VisualKind::Cube,
            ContributorRole::Primary,
        ),
    ]
    .map(|(flags, kind, contributor_role)| BlockVisual {
        support: assets::VisualSupport::Exact,
        faces: [0; 6],
        flags,
        kind,
        contributor_role,
        model_template: NO_MODEL_TEMPLATE,
        animation: NO_ANIMATION,
        variant: 0,
    });
    let compiled = CompiledAssets {
        visuals: visuals.into(),
        light_properties: vec![
            LightProperties::new(0, 0).unwrap(),
            LightProperties::new(15, 0).unwrap(),
            LightProperties::new(0, 15).unwrap(),
            LightProperties::new(0, 0).unwrap(),
            LightProperties::new(0, 2).unwrap(),
        ]
        .into_boxed_slice(),
        hashed: vec![(100, 0), (101, 1), (102, 2), (103, 3), (104, 4)].into_boxed_slice(),
        materials: vec![Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        }]
        .into_boxed_slice(),
        model_templates: Box::new([]),
        model_quads: Box::new([]),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16_u32, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![0xff; size as usize * size as usize * 4].into_boxed_slice(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })]
        .into_boxed_slice(),
        biomes: CompiledBiomeAssets::diagnostic(),
        provenance: assets::BlobProvenance {
            source_manifest_sha256: [0xA5; 32],
            block_registry_sha256: [0x5A; 32],
            light_registry_sha256: [0x33; 32],
            biome_registry_sha256: [0x3C; 32],
        },
    };
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

fn install_source(
    stream: &mut WorldStream,
    key: SubChunkKey,
    block: u8,
    sky: u8,
    direct: bool,
) -> [bool; 6] {
    install_current_light(stream, key, 0, 0, false);
    let generation = stream.light_store.light(key).unwrap().generation();
    let replacement = SubChunkLight::uniform(block, sky, generation).unwrap();
    let replacement_direct = DirectSkyMask::Uniform(direct);
    let monotonic_faces = stream.monotonic_light_faces(key, &replacement, &replacement_direct);
    stream.light_store.insert_known_air(key, replacement);
    stream.direct_sky.get_mut(&key).unwrap().mask = Arc::new(replacement_direct);
    monotonic_faces
}

fn install_resident(
    stream: &mut WorldStream,
    key: SubChunkKey,
    sub_chunk: SubChunk,
    block: u8,
    sky: u8,
    direct: bool,
) {
    stream.store.commit_sub_chunk(key, sub_chunk).unwrap();
    install_current_light(stream, key, block, sky, direct);
}

fn layered_uniform_sub_chunk(runtime_ids: &[u32]) -> SubChunk {
    let mut bytes = vec![8, runtime_ids.len() as u8];
    for &runtime_id in runtime_ids {
        bytes.push(1);
        bytes.extend(super::zig_zag_i32(runtime_id as i32));
    }
    SubChunk::decode(&bytes).expect("decode layered uniform test subchunk")
}

#[test]
fn resident_filter_refines_only_failed_unit_attenuation_cells() {
    let mut stream = filtered_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let destination = SubChunkKey::new(1, 1, 0, 0);
    let monotonic_faces = install_source(&mut stream, source, 15, 0, false);

    install_resident(
        &mut stream,
        destination,
        super::uniform_sub_chunk(2),
        0,
        0,
        false,
    );
    assert!(stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));

    install_resident(
        &mut stream,
        destination,
        super::uniform_sub_chunk(4),
        13,
        0,
        false,
    );
    assert!(stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));

    stream
        .store
        .update_block(destination, BlockUpdate::new(0, 0, 0, 0, 3), 0)
        .unwrap();
    install_current_light(&mut stream, destination, 13, 0, false);
    assert!(!stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));
}

#[test]
fn downward_direct_sky_requires_provenance_only_through_filter_zero() {
    let mut stream = filtered_stream(0);
    let source = SubChunkKey::new(0, 0, 1, 0);
    let destination = SubChunkKey::new(0, 0, 0, 0);
    let monotonic_faces = install_source(&mut stream, source, 0, 15, true);

    install_resident(
        &mut stream,
        destination,
        super::uniform_sub_chunk(2),
        0,
        0,
        false,
    );
    assert!(stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));

    install_resident(
        &mut stream,
        destination,
        super::uniform_sub_chunk(4),
        0,
        13,
        false,
    );
    assert!(stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));
    install_current_light(&mut stream, destination, 0, 12, false);
    assert!(!stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));

    install_resident(
        &mut stream,
        destination,
        super::uniform_sub_chunk(3),
        0,
        15,
        false,
    );
    assert!(!stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));
    install_current_light(&mut stream, destination, 0, 15, true);
    assert!(stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));
}

#[test]
fn resident_filter_matches_layer_max_and_network_identity_modes() {
    let source = SubChunkKey::new(1, 0, 0, 0);
    let destination = SubChunkKey::new(1, 1, 0, 0);
    for (layers, destination_level) in [([2, 3], 0), ([3, 2], 0), ([4, 3], 13), ([3, 4], 13)] {
        let mut stream = filtered_stream(1);
        let monotonic_faces = install_source(&mut stream, source, 15, 0, false);
        install_resident(
            &mut stream,
            destination,
            layered_uniform_sub_chunk(&layers),
            destination_level,
            0,
            false,
        );
        assert!(stream.current_known_target_dominates_source_face(
            source,
            destination,
            monotonic_faces
        ));
    }

    for (hashed, filter_two, air) in [(false, 4, 0), (true, 104, 100)] {
        let mut stream = filtered_stream(1);
        if hashed {
            stream.network_id_mode = NetworkIdMode::Hashed;
            stream.classifier = BlockClassifier::new(air);
        }
        let monotonic_faces = install_source(&mut stream, source, 15, 0, false);
        install_resident(
            &mut stream,
            destination,
            super::uniform_sub_chunk(filter_two),
            13,
            0,
            false,
        );
        assert!(stream.current_known_target_dominates_source_face(
            source,
            destination,
            monotonic_faces
        ));

        install_resident(
            &mut stream,
            destination,
            super::uniform_sub_chunk(air),
            13,
            0,
            false,
        );
        assert!(!stream.current_known_target_dominates_source_face(
            source,
            destination,
            monotonic_faces
        ));
    }
}

#[test]
fn unknown_runtime_and_unknown_destination_keep_distinct_fallbacks() {
    let mut stream = filtered_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let destination = SubChunkKey::new(1, 1, 0, 0);
    let monotonic_faces = install_source(&mut stream, source, 15, 0, false);
    install_resident(
        &mut stream,
        destination,
        super::uniform_sub_chunk(99_999),
        0,
        0,
        false,
    );
    let missing_before = stream.runtime_assets.missing_count();
    assert!(stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));
    assert!(stream.runtime_assets.missing_count() > missing_before);

    let unknown = SubChunkKey::new(1, 0, 0, 1);
    let missing_before = stream.runtime_assets.missing_count();
    assert!(!stream.current_known_target_dominates_source_face(source, unknown, monotonic_faces));
    assert_eq!(stream.runtime_assets.missing_count(), missing_before);
}

#[test]
fn successful_unit_bound_does_not_resolve_an_unknown_palette() {
    let mut stream = filtered_stream(1);
    let source = SubChunkKey::new(1, 0, 0, 0);
    let destination = SubChunkKey::new(1, 1, 0, 0);
    let monotonic_faces = install_source(&mut stream, source, 15, 0, false);
    install_resident(
        &mut stream,
        destination,
        super::uniform_sub_chunk(99_999),
        14,
        0,
        false,
    );
    let missing_before = stream.runtime_assets.missing_count();

    assert!(stream.current_known_target_dominates_source_face(
        source,
        destination,
        monotonic_faces
    ));
    assert_eq!(stream.runtime_assets.missing_count(), missing_before);
}
