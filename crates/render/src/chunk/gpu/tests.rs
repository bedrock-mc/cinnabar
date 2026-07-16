use super::*;
use crate::chunk::{
    gpu::upload::validate_local_model_streams,
    transparent::model::sorted_transparent_model_draw_words,
};

#[test]
fn chunk_sampler_keeps_native_texels_crisp_without_discarding_minification_mips() {
    let descriptor = chunk_sampler_descriptor();
    assert_eq!(descriptor.mag_filter, FilterMode::Nearest);
    assert_eq!(descriptor.min_filter, FilterMode::Linear);
    assert_eq!(descriptor.mipmap_filter, FilterMode::Linear);
    assert_eq!(descriptor.anisotropy_clamp, 1);
}

#[test]
fn gpu_binding_and_pipeline_owners_are_global_resources() {
    fn assert_resource<T: Resource>() {}
    assert_resource::<ChunkGpuArena>();
    assert_resource::<ChunkPipeline>();
}

#[test]
fn allocation_is_atomic_across_streams() {
    let free_cube = std::iter::once(0..1).collect::<Vec<_>>();
    let required = GeometryStreamCounts {
        cube: 1,
        cube_lighting: 1,
        model: 1,
        model_lighting: 1,
        model_draw: 1,
        transparent_model_draw: 0,
        liquid: 1,
        liquid_lighting: 1,
    };
    let plan = plan_chunk_range_update(
        1,
        &free_cube,
        0,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 1,
            max_geometry_stream_words: 15,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    );

    assert!(plan.is_none());
    assert_eq!(free_cube.len(), 1);
    assert_eq!(free_cube[0].start, 0);
    assert_eq!(free_cube[0].end, 1);

    let retry = plan_chunk_range_update(
        1,
        &free_cube,
        0,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 1,
            max_geometry_stream_words: 16,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("the unchanged state retries once every stream can fit");
    assert_eq!(retry.quad_start, 0);
    assert_eq!(retry.model_start, 0);
    assert_eq!(retry.model_lighting_start, 4);
    assert_eq!(retry.model_draw_start, 6);
    assert_eq!(retry.liquid_start, 8);
    assert_eq!(retry.liquid_lighting_start, 12);
    assert_eq!(retry.cube_lighting_start, 14);
    assert_eq!(retry.geometry_stream_capacity, 16);

    let empty = plan_chunk_range_update(
        0,
        &[],
        0,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        GeometryStreamCounts::default(),
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 0,
            max_geometry_stream_words: 0,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("empty streams need no geometry arena capacity");
    assert_eq!(empty.quad_capacity, 0);
    assert_eq!(empty.geometry_stream_capacity, 0);
}

#[test]
fn shared_geometry_layout_aligns_liquid_records_after_odd_model_lighting() {
    let required = GeometryStreamCounts {
        model: 1,
        model_lighting: 1,
        model_draw: 1,
        liquid: 1,
        liquid_lighting: 1,
        ..Default::default()
    };
    let plan = plan_chunk_range_update(
        0,
        &[],
        0,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 0,
            max_geometry_stream_words: 16,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("the aligned shared streams fit the arena");

    assert_eq!(plan.liquid_start % 4, 0);
    assert_eq!(plan.model_draw_start, plan.model_lighting_start + 2);
    assert_eq!(plan.liquid_lighting_start % 2, 0);
}

#[test]
fn shared_geometry_tail_alignment_is_included_in_arena_accounting() {
    let required = GeometryStreamCounts {
        liquid: 1,
        liquid_lighting: 1,
        ..Default::default()
    };
    let plan = plan_chunk_range_update(
        0,
        &[],
        2,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 0,
            max_geometry_stream_words: 10,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("two padding words plus six stream words fit exactly");

    assert_eq!(plan.geometry_stream_start, 4);
    assert_eq!(plan.liquid_start, 4);
    assert_eq!(plan.liquid_lighting_start, 8);
    assert_eq!(plan.geometry_stream_capacity, 6);
    assert_eq!(plan.geometry_stream_len, 10);
    assert_eq!(plan.free_geometry_stream_words, vec![2..4]);
}

#[test]
fn aligned_shared_geometry_reuses_an_eligible_old_allocation() {
    let required = GeometryStreamCounts {
        model: 1,
        model_lighting: 1,
        model_draw: 1,
        liquid: 1,
        liquid_lighting: 1,
        ..Default::default()
    };
    let mut old = retirement_test_allocation();
    old.model_range = Some(8..12);
    old.model_lighting_range = Some(12..14);
    old.model_draw_range = Some(14..16);
    old.liquid_range = Some(16..20);
    old.liquid_lighting_range = Some(20..22);
    old.geometry_stream_range = Some(8..22);
    old.geometry_stream_capacity = 14;
    let plan = plan_chunk_range_update(
        0,
        &[],
        22,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        Some(&old),
        false,
        ArenaLimits {
            max_quad_items: 0,
            max_geometry_stream_words: 22,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("the aligned old allocation remains reusable");

    assert_eq!(plan.geometry_stream_start, 8);
    assert_eq!(plan.geometry_stream_capacity, 14);
    assert_eq!(plan.liquid_start, 16);
    assert_eq!(plan.geometry_stream_len, 22);
    assert!(plan.free_geometry_stream_words.is_empty());
}

#[test]
fn aligned_shared_geometry_is_transparent_validator_eligible() {
    let key = SubChunkKey::new(0, 1, 4, 5);
    let tint = ChunkBiomeTintIdentity::new(2, 3);
    let required = GeometryStreamCounts {
        model: 1,
        model_lighting: 1,
        liquid: 1,
        liquid_lighting: 1,
        ..Default::default()
    };
    let plan = plan_chunk_range_update(
        0,
        &[],
        0,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 0,
            max_geometry_stream_words: 14,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("aligned streams fit exactly");
    let instance = ChunkRenderInstance {
        key,
        cube_quads: Arc::from([]),
        cube_lighting: Arc::from([]),
        model_refs: Arc::from([]),
        model_lighting: Arc::from([]),
        model_draw_refs: Arc::from([]),
        transparent_model_draw_refs: Arc::from([]),
        liquid_quads: Arc::from([PackedLiquidQuad::try_pack(
            [0, 0, 0],
            Face::PositiveY,
            [255; 4],
            1,
            0,
            [0, 0],
            false,
        )
        .unwrap()]),
        liquid_lighting: Arc::from([PackedQuadLighting::new([0; 4])]),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_start: None,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: tint,
        generation: 9,
        token: None,
        origin: [0; 3],
    };
    let allocation = GpuChunkAllocation {
        key,
        generation: 9,
        tint_identity: tint,
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: Some(plan.model_start..plan.model_lighting_start),
        model_lighting_range: Some(plan.model_lighting_start..plan.liquid_start),
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: Some(plan.liquid_start..plan.liquid_start + 4),
        liquid_lighting_range: Some(plan.liquid_lighting_start..plan.liquid_lighting_start + 2),
        has_depth_liquid: false,
        has_transparent_liquid: true,
        depth_liquid_range: None,
        metadata_index: 0,
    };

    assert!(transparent_allocation_matches(&instance, &allocation, tint));
}

#[test]
fn presentation_waits_for_expected_stream_mask() {
    let now = Instant::now();
    let key = SubChunkKey::new(0, 65, 0, 65);
    let entity = Entity::from_bits(1 << 32 | 1);
    let identity = FrameAllocationIdentity {
        entity,
        key,
        generation: 7,
    };
    let probe = FrameProbe::begin(
        target_expectation(now, [(key, 7)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        }],
        [(identity, ChunkStreamMask::MODEL | ChunkStreamMask::LIQUID)],
    );

    assert!(probe.record_direct_streams(entity, identity, ChunkStreamMask::MODEL));
    assert!(probe.complete().drawn_manifest.is_empty());

    let probe = FrameProbe::begin(
        target_expectation(now, [(key, 7)]),
        [FrameInstanceIdentity {
            entity,
            key,
            generation: 7,
        }],
        [(identity, ChunkStreamMask::MODEL | ChunkStreamMask::LIQUID)],
    );
    assert!(probe.record_direct_streams(
        entity,
        identity,
        ChunkStreamMask::MODEL | ChunkStreamMask::LIQUID,
    ));
    assert_eq!(probe.complete().drawn_manifest.as_ref(), &[(key, 7)]);
}

fn normalize_source_newlines(source: &str) -> String {
    source.replace("\r\n", "\n")
}

#[test]
fn source_parser_normalizes_crlf_for_windows_worktrees() {
    assert_eq!(
        normalize_source_newlines("first\r\nsecond\r\n"),
        "first\nsecond\n"
    );
}

#[test]
fn presentation_completion_uses_keyed_expected_mask_lookup() {
    let source = normalize_source_newlines(include_str!("../presentation/frame_probe.rs"));
    let complete = source
        .split_once("    pub(in crate::chunk) fn complete(self) -> CompletedFrameProbe {")
        .expect("frame probe completion")
        .1
        .split_once("\n    }\n}\n\n#[derive(Default)]")
        .expect("end of frame probe completion")
        .0;

    assert!(
        !complete.contains("self.eligible.values().find_map"),
        "completion must not linearly scan every eligible allocation per drawn identity"
    );
}

#[test]
fn realizable_packed_model_upload_addresses_are_identical_for_direct_and_mdi() {
    let model_quad_counts = [2_u32, 12, 20];
    let required = GeometryStreamCounts {
        model: model_quad_counts.len() as u32,
        model_lighting: model_quad_counts.iter().sum(),
        model_draw: model_quad_counts.iter().sum(),
        ..Default::default()
    };
    let plan = plan_chunk_range_update(
        0,
        &[],
        0,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 0,
            max_geometry_stream_words: 192,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("three packed model refs and all lighting sidecars fit");
    let model_range = checked_geometry_range(plan.model_start, required.model * 4).unwrap();
    let model_lighting_range =
        checked_geometry_range(plan.model_lighting_start, required.model_lighting * 2).unwrap();
    let model_draw_range =
        checked_geometry_range(plan.model_draw_start, required.model_draw * 2).unwrap();
    let allocation = GpuChunkAllocation {
        key: SubChunkKey::new(0, 0, 0, 0),
        generation: 1,
        tint_identity: ChunkBiomeTintIdentity::default(),
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: Some(model_range.clone()),
        model_lighting_range: Some(model_lighting_range.clone()),
        model_draw_range: Some(model_draw_range.clone()),
        transparent_model_draw_range: None,
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        metadata_index: 3,
    };

    let direct_addresses = direct_stream_addresses(&allocation);
    let mdi_addresses = mdi_stream_addresses(&allocation);
    assert_eq!(direct_addresses, mdi_addresses);
    assert_eq!(direct_addresses.model, Some(model_range.clone()));
    assert_eq!(
        direct_addresses.model_lighting,
        Some(model_lighting_range.clone())
    );
    assert_eq!(direct_addresses.model_draw, Some(model_draw_range.clone()));

    let mut refs = Vec::new();
    let mut relative_lighting_base = 0;
    for (template, quad_count) in model_quad_counts.into_iter().enumerate() {
        refs.push([0x888, template as u32, relative_lighting_base, u32::MAX]);
        relative_lighting_base += quad_count;
    }
    absolutize_model_lighting_bases(&mut refs, plan.model_lighting_start);
    let lighting_record_range = model_lighting_range.start / 2..model_lighting_range.end / 2;
    assert_eq!(
        refs.iter().map(|words| words[2]).collect::<Vec<_>>(),
        vec![
            lighting_record_range.start,
            lighting_record_range.start + 2,
            lighting_record_range.start + 14,
        ]
    );
    assert!(
        refs.iter()
            .all(|words| lighting_record_range.contains(&words[2]))
    );

    let direct = model_direct_draw_command(&allocation).expect("direct model draw");
    let mdi = model_mdi_draw_command(&allocation).expect("MDI model draw");
    assert_eq!(direct.index_count, mdi.index_count);
    assert_eq!(direct.instance_count, mdi.instance_count);
    assert_eq!(direct.first_index, mdi.first_index);
    assert_eq!(direct.base_vertex, mdi.base_vertex);
    assert_eq!(direct.first_instance, mdi.first_instance);
    assert_eq!(direct.index_count, 6);
    assert_eq!(
        direct.instance_count,
        model_quad_counts.iter().copied().sum::<u32>()
    );
    assert_eq!(direct.first_instance, model_draw_range.start / 2);
    assert_eq!(direct.base_vertex, 3 * 4);

    for malformed in [
        GpuChunkAllocation {
            model_range: None,
            ..allocation.clone()
        },
        GpuChunkAllocation {
            model_lighting_range: None,
            ..allocation.clone()
        },
        GpuChunkAllocation {
            model_draw_range: None,
            ..allocation.clone()
        },
        GpuChunkAllocation {
            model_draw_range: Some(model_draw_range.start + 2..model_draw_range.end + 2),
            ..allocation.clone()
        },
        GpuChunkAllocation {
            model_draw_range: Some(model_draw_range.start + 1..model_draw_range.end),
            ..allocation.clone()
        },
    ] {
        assert!(model_direct_draw_command(&malformed).is_none());
        assert!(model_mdi_draw_command(&malformed).is_none());
    }
}

#[test]
fn model_lighting_base_patches_to_shared_arena_records_without_mutating_other_words() {
    let mut refs = vec![[0x432, 7, 0, 0b11], [0x765, 8, 2, 0b101]];
    absolutize_model_lighting_bases(&mut refs, 20);
    assert_eq!(refs, [[0x432, 7, 10, 0b11], [0x765, 8, 12, 0b101]]);
}

#[test]
fn model_upload_validation_requires_an_exact_reachable_triplet() {
    let templates = [assets::ModelTemplate {
        quad_start: 0,
        quad_count: 6,
        flags: 0,
    }];
    let refs = [PackedModelRef::new(0x432, 0, 0, 0b10_1101)];
    let lighting = [PackedQuadLighting::new([0; 4]); 6];
    let draws = [
        PackedModelDrawRef::new(0, 0),
        PackedModelDrawRef::new(0, 2),
        PackedModelDrawRef::new(0, 3),
        PackedModelDrawRef::new(0, 5),
    ];
    assert!(validate_local_model_streams(
        &refs, &lighting, &draws, &templates
    ));
    assert!(!validate_local_model_streams(
        &refs,
        &lighting,
        &[],
        &templates
    ));
    assert!(!validate_local_model_streams(
        &[],
        &lighting,
        &draws,
        &templates
    ));
    assert!(!validate_local_model_streams(
        &refs,
        &[],
        &draws,
        &templates
    ));
    assert!(!validate_local_model_streams(
        &refs,
        &lighting,
        &[PackedModelDrawRef::new(1, 0)],
        &templates,
    ));
    assert!(!validate_local_model_streams(
        &refs,
        &lighting,
        &[PackedModelDrawRef::new(0, 32)],
        &templates,
    ));
    assert!(!validate_local_model_streams(
        &refs,
        &lighting,
        &[PackedModelDrawRef::new(0, 1)],
        &templates,
    ));
    assert!(!validate_local_model_streams(
        &[PackedModelRef::new(0x432, 0, 5, 0b10_0000)],
        &lighting,
        &[PackedModelDrawRef::new(0, 5)],
        &templates,
    ));
}

#[test]
fn transparent_model_draw_uses_only_its_exact_partitioned_range() {
    let required = GeometryStreamCounts {
        model: 1,
        model_lighting: 2,
        model_draw: 1,
        transparent_model_draw: 2,
        ..Default::default()
    };
    let plan = plan_chunk_range_update(
        0,
        &[],
        0,
        &[],
        FALLBACK_BIOME_WORDS,
        &[],
        required,
        0,
        None,
        false,
        ArenaLimits {
            max_quad_items: 0,
            max_geometry_stream_words: 32,
            max_origin_items: 1,
            max_biome_words: FALLBACK_BIOME_WORDS,
        },
    )
    .expect("partitioned model upload fits");
    let transparent_range = plan.transparent_model_draw_start
        ..plan.transparent_model_draw_start + required.transparent_model_draw * 2;
    let allocation = GpuChunkAllocation {
        key: SubChunkKey::new(0, 0, 0, 0),
        generation: 1,
        tint_identity: ChunkBiomeTintIdentity::default(),
        quad_range: 0..0,
        cube_lighting_range: None,
        model_range: Some(plan.model_start..plan.model_lighting_start),
        model_lighting_range: Some(plan.model_lighting_start..plan.model_draw_start),
        model_draw_range: Some(plan.model_draw_start..plan.transparent_model_draw_start),
        transparent_model_draw_range: Some(transparent_range.clone()),
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        metadata_index: 3,
    };

    let draw =
        transparent_model_direct_draw_command(&allocation).expect("transparent model draw command");
    assert_eq!(draw.index_count, MODEL_INDEX_COUNT);
    assert_eq!(draw.instance_count, 2);
    assert_eq!(draw.first_instance, transparent_range.start / 2);
    assert_eq!(draw.base_vertex, 12);
    assert!(
        transparent_model_direct_draw_command(&GpuChunkAllocation {
            transparent_model_draw_range: None,
            ..allocation
        })
        .is_none()
    );
}

#[test]
fn transparent_model_phase_distance_is_monotonic_by_subchunk_center() {
    let rangefinder = bevy::render::render_phase::ViewRangefinder3d::from_world_from_view(
        &bevy::math::Affine3A::from_translation(Vec3::new(0.0, 0.0, -1.0)),
    );
    let near = SubChunkKey::new(0, 0, 0, 0);
    let far = SubChunkKey::new(0, 0, 0, 2);

    assert_eq!(transparent_model_subchunk_center(near), Vec3::splat(8.0));
    assert!(
        transparent_model_phase_distance(&rangefinder, far)
            > transparent_model_phase_distance(&rangefinder, near)
    );
}

#[test]
fn transparent_liquid_groups_share_the_model_subchunk_distance_contract() {
    let near =
        TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 0), 1, 8..12, 24..26, 10);
    let far = TransparentAllocationIdentity::new(SubChunkKey::new(0, 0, 0, 2), 2, 0..8, 20..24, 20);
    let key = ViewSortKey::try_new(
        [0.0; 3],
        [0.0, 0.0, 0.0, 1.0],
        vec![near.clone(), far.clone()],
        ChunkTextureAssetIdentity::new(1, 1),
        ChunkBiomeTintIdentity::new(1, 1),
    )
    .unwrap();
    let snapshot = committed_transparent_state(
        &key,
        vec![
            PackedTransparentDrawRef::new(0, far.metadata_index),
            PackedTransparentDrawRef::new(1, far.metadata_index),
            PackedTransparentDrawRef::new(2, near.metadata_index),
        ],
    )
    .committed()
    .unwrap()
    .clone();

    let groups = transparent_liquid_phase_groups(&snapshot).expect("exact grouped snapshot");
    assert_eq!(
        groups,
        [
            TransparentLiquidPhaseGroup {
                key: far.key,
                ref_range: 0..2,
            },
            TransparentLiquidPhaseGroup {
                key: near.key,
                ref_range: 2..3,
            },
        ]
    );
    let rangefinder = bevy::render::render_phase::ViewRangefinder3d::from_world_from_view(
        &bevy::math::Affine3A::from_translation(Vec3::new(0.0, 0.0, -1.0)),
    );
    assert_eq!(
        transparent_liquid_phase_distance(&rangefinder, groups[0].key),
        transparent_model_phase_distance(&rangefinder, far.key),
    );
    assert!(
        transparent_liquid_phase_distance(&rangefinder, groups[0].key)
            > transparent_liquid_phase_distance(&rangefinder, groups[1].key)
    );
    assert_eq!(
        transparent_draw_range_args(snapshot.buffer_slot(), groups[0].ref_range.clone()),
        Some(TransparentDrawArgs {
            index_count: 6,
            instance_count: 2,
            first_index: 0,
            base_vertex: 0,
            first_instance: 0,
        })
    );

    let mut non_contiguous = snapshot;
    non_contiguous.refs = Arc::from([
        PackedTransparentDrawRef::new(0, far.metadata_index),
        PackedTransparentDrawRef::new(2, near.metadata_index),
        PackedTransparentDrawRef::new(1, far.metadata_index),
    ]);
    assert!(transparent_liquid_phase_groups(&non_contiguous).is_none());
    assert!(transparent_draw_range_args(0, 0..MAX_TRANSPARENT_DRAW_REFS as u32 + 1).is_none());
}

#[test]
fn transparent_model_face_order_reverses_with_camera_rotation() {
    let model_refs = [PackedModelRef::new(0, 0, 0, 0b11)];
    let draw_refs = [PackedModelDrawRef::new(0, 0), PackedModelDrawRef::new(0, 1)];
    let templates = [assets::ModelTemplate {
        quad_start: 0,
        quad_count: 2,
        flags: 0,
    }];
    let quads = [
        assets::ModelQuad {
            positions: [[0, 0, 0], [256, 0, 0], [256, 256, 0], [0, 256, 0]],
            uvs: [[0; 2]; 4],
            material: 0,
            flags: 0,
        },
        assets::ModelQuad {
            positions: [[0, 0, 256], [0, 256, 256], [256, 256, 256], [256, 0, 256]],
            uvs: [[0; 2]; 4],
            material: 0,
            flags: 0,
        },
    ];
    let identity_view = ViewRangefinder3d::from_world_from_view(&bevy::math::Affine3A::IDENTITY);
    let reversed_view =
        ViewRangefinder3d::from_world_from_view(&bevy::math::Affine3A::from_rotation_translation(
            Quat::from_rotation_y(std::f32::consts::PI),
            Vec3::ZERO,
        ));

    assert_eq!(
        sorted_transparent_model_draw_words(
            &identity_view,
            SubChunkKey::new(0, 0, 0, 0),
            &model_refs,
            &draw_refs,
            &templates,
            &quads,
            5,
        )
        .unwrap(),
        [[5, 0], [5, 1]],
    );
    assert_eq!(
        sorted_transparent_model_draw_words(
            &reversed_view,
            SubChunkKey::new(0, 0, 0, 0),
            &model_refs,
            &draw_refs,
            &templates,
            &quads,
            5,
        )
        .unwrap(),
        [[5, 1], [5, 0]],
    );

    let entity = Entity::from_bits(1);
    let candidates = Arc::from(
        draw_refs
            .iter()
            .copied()
            .enumerate()
            .map(|(stable_index, draw_ref)| {
                let (centroid, words) = transparent_model_draw_candidate(
                    SubChunkKey::new(0, 0, 0, 0),
                    &model_refs,
                    draw_ref,
                    &templates,
                    &quads,
                    5,
                )
                .unwrap();
                TransparentModelSortCandidate {
                    entity,
                    key: SubChunkKey::new(0, 0, 0, 0),
                    draw_range: 20..24,
                    stable_index: stable_index as u32,
                    centroid,
                    words,
                }
            })
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        sort_transparent_model_candidates(Mat4::IDENTITY, Arc::clone(&candidates))[0]
            .words
            .as_ref(),
        [[5, 0], [5, 1]],
    );
    assert_eq!(
        sort_transparent_model_candidates(
            Mat4::from_quat(Quat::from_rotation_y(std::f32::consts::PI)),
            candidates,
        )[0]
        .words
        .as_ref(),
        [[5, 1], [5, 0]],
    );
}

#[test]
fn transparent_model_upload_batches_respect_cap_without_splitting_subchunks() {
    let mut batches = VecDeque::from([
        TransparentModelSortBatch {
            draw_range: 0..6,
            words: vec![[0, 0]; 3].into_boxed_slice(),
        },
        TransparentModelSortBatch {
            draw_range: 6..14,
            words: vec![[1, 0]; 4].into_boxed_slice(),
        },
    ]);

    let first = take_transparent_model_upload_batches(&mut batches, 5);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].draw_range, 0..6);
    assert_eq!(batches.len(), 1);

    let second = take_transparent_model_upload_batches(&mut batches, 5);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].draw_range, 6..14);
    assert!(batches.is_empty());
}

#[test]
fn transparent_model_rotation_cache_key_normalizes_quaternion_sign() {
    let rotation = Quat::from_rotation_y(0.75);
    assert_eq!(
        canonical_transparent_rotation_bits(rotation),
        canonical_transparent_rotation_bits(-rotation),
    );
    assert_ne!(
        canonical_transparent_rotation_bits(rotation),
        canonical_transparent_rotation_bits(Quat::from_rotation_y(1.0)),
    );
}

#[test]
fn shared_geometry_layout_keeps_both_model_draw_routes_contiguous_and_bounded() {
    let layout = GeometryStreamCounts {
        cube: 3,
        cube_lighting: 3,
        model: 1,
        model_lighting: 2,
        model_draw: 1,
        transparent_model_draw: 2,
        liquid: 1,
        liquid_lighting: 1,
    }
    .layout()
    .expect("bounded mixed model streams");

    assert_eq!(layout.model_offset, 0);
    assert_eq!(layout.model_lighting_offset, 4);
    assert_eq!(layout.model_draw_offset, 8);
    assert_eq!(layout.transparent_model_draw_offset, 10);
    assert_eq!(layout.liquid_offset, 16);
    assert_eq!(layout.liquid_lighting_offset, 20);
    assert_eq!(layout.cube_lighting_offset, 22);
    assert_eq!(layout.word_count, 28);
    assert_eq!(layout.cube_lighting_offset % 2, 0);
}

#[test]
fn cube_lighting_layout_rejects_overflow_and_origin_abi_carries_both_bases() {
    assert_eq!(std::mem::size_of::<GpuChunkOrigin>(), 32);
    assert_eq!(CHUNK_ORIGIN_BYTES, 32);
    let origin = gpu_chunk_origin([1, -2, 3], 7, 11, 24).expect("even light word base");
    assert_eq!(origin.value, [1, -2, 3, 7]);
    assert_eq!(origin.cube_bases, [11, 12, 0, 0]);
    assert!(gpu_chunk_origin([0; 3], 0, 0, 3).is_none());
    assert!(
        GeometryStreamCounts {
            cube_lighting: u32::MAX,
            ..default()
        }
        .layout()
        .is_none()
    );
}

#[test]
fn direct_and_mdi_cube_lighting_addresses_resolve_identical_sentinels() {
    let allocation = GpuChunkAllocation {
        key: SubChunkKey::new(0, 0, 0, 0),
        generation: 1,
        tint_identity: ChunkBiomeTintIdentity::default(),
        quad_range: 11..13,
        cube_lighting_range: Some(24..28),
        model_range: None,
        model_lighting_range: None,
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        metadata_index: 4,
    };
    let direct = direct_stream_addresses(&allocation);
    let mdi = mdi_stream_addresses(&allocation);
    assert_eq!(direct, mdi);
    assert_eq!(cube_lighting_record_address(&direct, 11), Some(12));
    assert_eq!(cube_lighting_record_address(&mdi, 12), Some(13));
    assert_eq!(cube_lighting_record_address(&direct, 10), None);

    let sentinel = [
        PackedQuadLighting::new([0x0011, 0x0022, 0x0033, 0x0044]),
        PackedQuadLighting::new([0x0111, 0x0222, 0x0333, 0x0444]),
    ];
    let local = cube_lighting_record_address(&direct, 12).unwrap() - 12;
    assert_eq!(sentinel[local as usize].samples()[3], 0x0444);

    let packed = packed_lighting_records(&sentinel);
    let layout = GeometryStreamCounts {
        cube: 2,
        cube_lighting: 2,
        liquid: 1,
        liquid_lighting: 1,
        ..default()
    }
    .layout()
    .unwrap();
    assert_eq!(layout.cube_lighting_offset, 6);
    let mut arena_words = vec![0_u32; layout.word_count as usize];
    let packed_words: &[u32] = bytemuck::cast_slice(&packed);
    arena_words[layout.cube_lighting_offset as usize..].copy_from_slice(packed_words);
    let record = cube_lighting_record_address(
        &StreamAddresses {
            cube: Some(11..13),
            cube_lighting: Some(6..10),
            ..default()
        },
        12,
    )
    .unwrap();
    let word = arena_words[(record * 2 + 1) as usize];
    assert_eq!(word >> 16, 0x0444);
}

#[test]
fn cube_draws_reject_missing_odd_mismatched_and_overlapping_lighting_ranges() {
    let valid = GpuChunkAllocation {
        key: SubChunkKey::new(0, 0, 0, 0),
        generation: 1,
        tint_identity: ChunkBiomeTintIdentity::default(),
        quad_range: 11..13,
        cube_lighting_range: Some(24..28),
        model_range: None,
        model_lighting_range: None,
        model_draw_range: None,
        transparent_model_draw_range: None,
        liquid_range: None,
        liquid_lighting_range: None,
        has_depth_liquid: false,
        has_transparent_liquid: false,
        depth_liquid_range: None,
        metadata_index: 4,
    };
    assert!(indexed_indirect_command(&valid).is_some());

    let mut missing = valid.clone();
    missing.cube_lighting_range = None;
    assert!(indexed_indirect_command(&missing).is_none());

    let mut odd = valid.clone();
    odd.cube_lighting_range = Some(25..29);
    assert!(indexed_indirect_command(&odd).is_none());

    let mut mismatched = valid.clone();
    mismatched.cube_lighting_range = Some(24..26);
    assert!(indexed_indirect_command(&mismatched).is_none());

    let mut overlapping = valid;
    overlapping.model_range = Some(26..30);
    assert!(indexed_indirect_command(&overlapping).is_none());
}

#[test]
fn model_upload_validation_enforces_exact_material_partition() {
    let templates = [assets::ModelTemplate {
        quad_start: 0,
        quad_count: 2,
        flags: 0,
    }];
    let quads = [
        assets::ModelQuad {
            positions: [[0; 3]; 4],
            uvs: [[0; 2]; 4],
            material: assets::DIAGNOSTIC_MATERIAL,
            flags: 0,
        },
        assets::ModelQuad {
            positions: [[0; 3]; 4],
            uvs: [[0; 2]; 4],
            material: 1,
            flags: 0,
        },
    ];
    let materials = [
        assets::Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
        },
        assets::Material {
            texture: TextureRef::DIAGNOSTIC,
            flags: assets::MATERIAL_FLAG_ALPHA_BLEND,
            animation: NO_ANIMATION,
        },
    ];
    let refs = [PackedModelRef::new(0x432, 0, 0, 0b11)];
    let lighting = [PackedQuadLighting::new([0; 4]); 2];
    let opaque = [PackedModelDrawRef::new(0, 0)];
    let blend = [PackedModelDrawRef::new(0, 1)];

    assert!(validate_partitioned_model_streams(
        &refs, &lighting, &opaque, &blend, &templates, &quads, &materials,
    ));
    assert!(!validate_partitioned_model_streams(
        &refs, &lighting, &blend, &opaque, &templates, &quads, &materials,
    ));
    assert!(!validate_partitioned_model_streams(
        &refs,
        &lighting,
        &opaque,
        &[],
        &templates,
        &quads,
        &materials,
    ));
}
