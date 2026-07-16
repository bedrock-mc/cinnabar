#[test]
fn one_opaque_block_emits_six_packed_quads() {
    let sub = blocks(7, &[[1, 2, 3]]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(mesh.quad_count(), 6);
    assert_eq!(mesh.quads().len(), 6);
    assert!(mesh.quads().iter().all(|quad| quad.origin() == [1, 2, 3]));
    assert!(mesh.quads().iter().all(|quad| quad.width() == 1));
    assert!(mesh.quads().iter().all(|quad| quad.height() == 1));
    assert!(mesh.quads().iter().all(|quad| quad.material_id() == 7));
    assert_eq!(mesh.quads()[0].face(), Face::NegativeX);
    assert_eq!(mesh.quads()[0].words(), [1 | (2 << 5) | (3 << 10), 7]);
    assert_eq!(mesh.cube_lighting().len(), mesh.cube_quads().len());
}

#[test]
fn cube_lighting_roundtrips_with_streams_and_rejects_count_mismatch() {
    let base = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &blocks(7, &[[1, 2, 3]]),
    );
    let quads = base.cube_quads().to_vec();
    let expected = (0..quads.len())
        .map(|index| PackedQuadLighting::new([index as u16 + 1; 4]))
        .collect::<Vec<_>>();
    let mesh = ChunkMesh::try_from_streams_with_cube_lighting(
        quads.clone(),
        expected.clone(),
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        base.connectivity(),
    )
    .expect("one cube-light sidecar per cube quad");
    let (roundtrip_quads, roundtrip_lighting, _, _, _, _, _, _) = mesh.into_streams();
    assert_eq!(roundtrip_quads.as_ref(), quads);
    assert_eq!(roundtrip_lighting.as_ref(), expected);

    let error = ChunkMesh::try_from_streams_with_cube_lighting(
        quads,
        expected[..expected.len() - 1].to_vec(),
        vec![],
        vec![],
        vec![],
        vec![],
        vec![],
        base.connectivity(),
    )
    .expect_err("cube lighting mismatch must fail closed");
    assert_eq!(error.cube_quads(), 6);
    assert_eq!(error.cube_lighting(), 5);
}

#[test]
fn cube_lighting_is_one_to_one_and_splits_greedy_runs() {
    let sub = blocks(11, &[[0, 0, 0], [1, 0, 0]]);
    let sampler =
        |[x, _y, _z]: [i32; 3]| MeshLightSample::try_new(if x <= 0 { 1 } else { 9 }, 15).unwrap();
    let mesh = mesh_sub_chunk_with_lighting(
        &classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
        &sampler,
    );

    assert_eq!(size_of::<PackedQuad>(), 8);
    assert_eq!(mesh.cube_lighting().len(), mesh.cube_quads().len());
    assert_eq!(
        mesh.quad_count(),
        10,
        "different packed light splits four coplanar runs"
    );
    assert!(mesh.quads().iter().all(|quad| quad.width() == 1));
    assert!(
        mesh.cube_lighting()
            .iter()
            .flat_map(|lighting| lighting.samples())
            .all(|sample| sample & 0x000f != 0 && (sample >> 4) & 0x000f == 15)
    );
}

#[test]
fn equal_adjacent_blocks_greedy_merge_into_six_prism_quads() {
    let sub = blocks(11, &[[0, 0, 0], [1, 0, 0]]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 6);
    assert_eq!(
        mesh.quads().iter().filter(|quad| quad.width() == 2).count(),
        4,
        "top, bottom, front, and back should span both X cells"
    );
}

#[test]
fn different_materials_split_coplanar_runs_but_still_cull_internal_faces() {
    let placements = [([0, 0, 0], 1), ([1, 0, 0], 2)];
    let sub = sub_chunk(vec![packed_storage(2, &[AIR, 13, 17], &placements)]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 10);
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == 13)
            .count(),
        5
    );
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == 17)
            .count(),
        5
    );
}

#[test]
fn asymmetric_internal_culling_uses_ordered_occluder_and_leaf_facts() {
    let cases = [
        (OPAQUE_A, OPAQUE_B, false, false, 10),
        (OPAQUE_A, LEAF_A, true, false, 11),
        (LEAF_A, OPAQUE_A, false, true, 11),
        (LEAF_A, LEAF_B, false, false, 10),
        (DIAGNOSTIC, LEAF_A, true, true, 12),
        (DIAGNOSTIC, OPAQUE_A, false, true, 11),
    ];

    for (source, neighbour, source_face, neighbour_face, total) in cases {
        let sub = adjacent_blocks(source, neighbour);
        let mesh = mesh(
            &classifier(),
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &sub,
        );

        assert_eq!(
            has_face(&mesh, [7, 8, 8], Face::PositiveX),
            source_face,
            "source={source} neighbour={neighbour}"
        );
        assert_eq!(
            has_face(&mesh, [8, 8, 8], Face::NegativeX),
            neighbour_face,
            "source={source} neighbour={neighbour}"
        );
        assert_eq!(
            mesh.quad_count(),
            total,
            "source={source} neighbour={neighbour}"
        );
    }
}

#[test]
fn asymmetric_boundary_culling_matches_internal_semantics_on_every_face() {
    let boundaries = [
        (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
        (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
        (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
        (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
        (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
        (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
    ];
    let pairs = [
        (OPAQUE_A, OPAQUE_B, 5),
        (OPAQUE_A, LEAF_A, 6),
        (LEAF_A, OPAQUE_A, 5),
        (LEAF_A, LEAF_B, 5),
        (DIAGNOSTIC, OPAQUE_A, 5),
        (DIAGNOSTIC, LEAF_A, 6),
        (DIAGNOSTIC, DIAGNOSTIC, 6),
        (OPAQUE_A, DIAGNOSTIC, 6),
        (LEAF_A, DIAGNOSTIC, 6),
    ];

    for (face, current_coordinate, neighbour_coordinate) in boundaries {
        for (source, neighbour_value, expected) in pairs {
            let sub = blocks(source, &[current_coordinate]);
            let neighbour = blocks(neighbour_value, &[neighbour_coordinate]);
            let neighbourhood = neighbourhood_for(face, &neighbour);
            let mesh = mesh(
                &classifier(),
                NetworkIdMode::Sequential,
                &neighbourhood,
                &sub,
            );

            assert_eq!(
                mesh.quad_count(),
                expected,
                "face={face:?} source={source} neighbour={neighbour_value}"
            );
            assert_eq!(
                has_face(&mesh, current_coordinate, face),
                expected == 6,
                "face={face:?} source={source} neighbour={neighbour_value}"
            );
        }
    }
}

#[test]
fn every_boundary_face_culls_against_its_cross_sub_chunk_neighbour() {
    let cases = [
        (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
        (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
        (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
        (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
        (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
        (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
    ];

    for (face, current_coordinate, neighbour_coordinate) in cases {
        let sub = blocks(23, &[current_coordinate]);
        let neighbour = blocks(23, &[neighbour_coordinate]);
        let neighbourhood = match face {
            Face::NegativeX => Neighbourhood::empty().with_negative_x(&neighbour),
            Face::PositiveX => Neighbourhood::empty().with_positive_x(&neighbour),
            Face::NegativeY => Neighbourhood::empty().with_negative_y(&neighbour),
            Face::PositiveY => Neighbourhood::empty().with_positive_y(&neighbour),
            Face::NegativeZ => Neighbourhood::empty().with_negative_z(&neighbour),
            Face::PositiveZ => Neighbourhood::empty().with_positive_z(&neighbour),
        };

        let mesh = mesh(
            &classifier(),
            NetworkIdMode::Sequential,
            &neighbourhood,
            &sub,
        );

        assert_eq!(mesh.quad_count(), 5, "failed to cull {face:?}");
        assert!(
            mesh.quads().iter().all(|quad| quad.face() != face),
            "retained cross-boundary {face:?}"
        );
    }
}

#[test]
fn zero_storage_and_uniform_air_emit_no_geometry() {
    let no_storage = sub_chunk(Vec::new());
    let uniform_air = uniform(AIR);

    let no_storage_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &no_storage,
    );
    let uniform_air_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &uniform_air,
    );

    assert!(no_storage_mesh.is_empty());
    assert!(uniform_air_mesh.is_empty());
    for face in Face::ALL {
        for other in Face::ALL {
            assert!(no_storage_mesh.connectivity().is_connected(face, other));
            assert!(uniform_air_mesh.connectivity().is_connected(face, other));
        }
    }
}

#[test]
fn layered_solid_and_water_are_both_resolved() {
    let solid = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]);
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![solid, water]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    );
    assert_eq!(resolved.palette_entry_count(), 4);
    let resolved = resolved.resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), Some(OPAQUE_A));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
    assert!(mesh.liquid_quads().is_empty());
    assert!(mesh.liquid_lighting().is_empty());
}

#[test]
fn allocation_free_single_coordinate_resolution_matches_cached_mixed_palettes() {
    let solid = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]);
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![solid, water]);
    let cached = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    let direct = ContributorResolver::resolve_direct(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
        [8, 8, 8],
    );

    assert_eq!(
        direct.primary_network_value(),
        cached.primary_network_value()
    );
    assert_eq!(direct.liquid_network_value(), cached.liquid_network_value());
    assert_eq!(
        direct.diagnostic_network_value(),
        cached.diagnostic_network_value()
    );
}

#[test]
fn layered_aquatic_cross_and_water_emit_model_without_diagnostic_cube() {
    let seagrass = packed_storage(1, &[AIR, CROSS], &[([8, 8, 8], 1)]);
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![seagrass, water]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), Some(CROSS));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert!(mesh.cube_quads().is_empty());
    assert_eq!(mesh.model_refs().len(), 1);
    assert_eq!(mesh.model_lighting().len(), 2);
    assert!(mesh.liquid_quads().is_empty());
    assert!(mesh.liquid_lighting().is_empty());
}

#[test]
fn uniform_solid_and_water_resolve_all_layers() {
    let sub = sub_chunk(vec![uniform_storage(OPAQUE_A), uniform_storage(LIQUID_A)]);
    let resolver = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    );
    assert_eq!(resolver.palette_entry_count(), 2);
    let resolved = resolver.resolve([15, 15, 15]);
    assert_eq!(resolved.primary_network_value(), Some(OPAQUE_A));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
}

#[test]
fn contributor_resolver_rejects_out_of_bounds_coordinates_consistently() {
    let uniform = uniform(OPAQUE_A);
    let mixed = blocks(OPAQUE_A, &[[8, 8, 8]]);
    for sub_chunk in [&uniform, &mixed] {
        let resolved = ContributorResolver::new(
            classifier(),
            runtime_assets(),
            NetworkIdMode::Sequential,
            sub_chunk,
        )
        .resolve([16, 0, 0]);
        assert_eq!(resolved.primary_network_value(), None);
        assert_eq!(resolved.liquid_network_value(), None);
        assert_eq!(resolved.diagnostic_network_value(), Some(0));
    }
}

#[test]
fn liquid_before_solid_is_order_independent() {
    let water = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let solid = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![water, solid]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), Some(OPAQUE_A));
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
}

#[test]
fn liquid_only_is_retained_without_diagnostic_cube() {
    let water = blocks(LIQUID_A, &[[8, 8, 8]]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &water,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &water,
    );

    assert!(mesh.cube_quads().is_empty());
    assert!(mesh.model_refs().is_empty());
}

#[test]
fn duplicate_liquid_collapses() {
    let first = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let duplicate = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![first, duplicate]);
    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), Some(LIQUID_A));
    assert_eq!(resolved.diagnostic_network_value(), None);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert!(mesh.cube_quads().is_empty());
    assert!(mesh.model_refs().is_empty());
    assert!(mesh.liquid_quads().is_empty());
    assert!(mesh.liquid_lighting().is_empty());
}

#[test]
fn two_primary_layers_fail_closed() {
    for second_runtime_id in [OPAQUE_A, OPAQUE_B] {
        let first = packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]);
        let second = packed_storage(1, &[AIR, second_runtime_id], &[([8, 8, 8], 1)]);
        let sub = sub_chunk(vec![first, second]);

        let resolved = ContributorResolver::new(
            classifier(),
            runtime_assets(),
            NetworkIdMode::Sequential,
            &sub,
        )
        .resolve([8, 8, 8]);
        assert_eq!(resolved.primary_network_value(), None);
        assert_eq!(resolved.liquid_network_value(), None);
        assert_eq!(resolved.diagnostic_network_value(), Some(second_runtime_id));

        assert_single_diagnostic_voxel(&sub);
    }
}

#[test]
fn distinct_liquids_fail_closed() {
    let first = packed_storage(1, &[AIR, LIQUID_A], &[([8, 8, 8], 1)]);
    let second = packed_storage(1, &[AIR, LIQUID_B], &[([8, 8, 8], 1)]);
    let sub = sub_chunk(vec![first, second]);

    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &sub,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), None);
    assert_eq!(resolved.diagnostic_network_value(), Some(LIQUID_B));

    assert_single_diagnostic_voxel(&sub);
}

#[test]
fn unsupported_additional_layer_fails_closed() {
    let unsupported = blocks(UNSUPPORTED_ADDITIONAL, &[[8, 8, 8]]);

    let resolved = ContributorResolver::new(
        classifier(),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &unsupported,
    )
    .resolve([8, 8, 8]);
    assert_eq!(resolved.primary_network_value(), None);
    assert_eq!(resolved.liquid_network_value(), None);
    assert_eq!(
        resolved.diagnostic_network_value(),
        Some(UNSUPPORTED_ADDITIONAL)
    );

    assert_single_diagnostic_voxel(&unsupported);
}

#[test]
fn sixteen_storage_layers_resolve_without_flattening() {
    let mut layers = vec![uniform_storage(AIR); world::MAX_STORAGE_COUNT - 1];
    layers.push(packed_storage(1, &[AIR, OPAQUE_A], &[([8, 8, 8], 1)]));
    let sub = sub_chunk(layers);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == OPAQUE_A)
    );
}

fn assert_single_diagnostic_voxel(sub_chunk: &SubChunk) {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        sub_chunk,
    );
    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
    assert!(mesh.model_refs().is_empty());
    assert!(mesh.liquid_quads().is_empty());
}

#[test]
fn debug_colours_are_deterministic_distinct_and_opaque() {
    assert_eq!(debug_color(0xdead_beef), debug_color(0xdead_beef));
    assert_ne!(debug_color(7), debug_color(8));
    assert_eq!(debug_color(7)[3], 255);
    assert_eq!(debug_color(u32::MAX)[3], 255);
}

#[test]
fn uniform_solid_fast_path_merges_planes_and_respects_boundary_neighbours() {
    let sub = uniform(37);
    let empty_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(empty_mesh.quad_count(), 6);
    assert!(
        empty_mesh
            .quads()
            .iter()
            .all(|quad| quad.width() == 16 && quad.height() == 16)
    );
    assert!(empty_mesh.connectivity().is_empty());

    let positive_x = uniform(41);
    let neighbourhood = Neighbourhood::empty().with_positive_x(&positive_x);
    let culled_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &neighbourhood,
        &sub,
    );

    assert_eq!(culled_mesh.quad_count(), 5);
    assert!(
        culled_mesh
            .quads()
            .iter()
            .all(|quad| quad.face() != Face::PositiveX)
    );
}

#[test]
fn uniform_leaf_meshes_outer_planes_but_is_cave_open() {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &uniform(LEAF_A),
    );

    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.width() == 16 && quad.height() == 16)
    );
    assert!(mesh.quads().iter().all(|quad| quad.material_id() == LEAF_A));
    assert!(mesh.connectivity().is_all_connected());
    assert_eq!(size_of::<PackedQuad>(), 8);
}

#[test]
fn uniform_diagnostic_emits_each_unculled_slice_and_is_cave_open() {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &uniform(DIAGNOSTIC),
    );

    assert_eq!(mesh.quad_count(), 96);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.width() == 16 && quad.height() == 16)
    );
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn emitted_diagnostic_quads_retain_sequential_identity_and_split_greedy_runs() {
    let sub = sub_chunk(vec![packed_storage(
        2,
        &[AIR, DIAGNOSTIC, 50_000],
        &[([0, 0, 0], 1), ([1, 0, 0], 2)],
    )]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 12);
    assert_eq!(
        mesh.diagnostic_geometry().entries(),
        &[
            meshing::DiagnosticGeometryCount::new(Some(DIAGNOSTIC), DIAGNOSTIC, 6),
            meshing::DiagnosticGeometryCount::new(None, 50_000, 6),
        ]
    );
    assert_eq!(mesh.diagnostic_geometry().omitted_identity_count(), 0);
    assert_eq!(mesh.diagnostic_geometry().omitted_quad_count(), 0);
}

#[test]
fn emitted_diagnostic_quads_retain_hash_and_resolved_sequential_identity() {
    let sub = sub_chunk(vec![packed_storage(
        1,
        &[0xdbf4_4120, 7],
        &[([4, 5, 6], 1)],
    )]);
    let mesh = mesh(
        &BlockClassifier::new(0xdbf4_4120),
        NetworkIdMode::Hashed,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(
        mesh.diagnostic_geometry().entries(),
        &[meshing::DiagnosticGeometryCount::new(Some(DIAGNOSTIC), 7, 6)]
    );
}

#[test]
fn real_geometry_never_enters_diagnostic_attribution() {
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &blocks(OPAQUE_A, &[[4, 5, 6]]),
    );

    assert!(mesh.diagnostic_geometry().entries().is_empty());
    assert_eq!(mesh.diagnostic_geometry().omitted_quad_count(), 0);
}

#[test]
fn per_mesh_diagnostic_summary_is_bounded_and_tie_ordered_by_identity() {
    let summary = meshing::DiagnosticGeometrySummary::from_counts(
        (0..meshing::MAX_DIAGNOSTIC_IDENTITIES_PER_MESH + 3)
            .rev()
            .map(|id| meshing::DiagnosticGeometryCount::new(Some(id as u32), id as u32, 1)),
    );

    assert_eq!(
        summary.entries().len(),
        meshing::MAX_DIAGNOSTIC_IDENTITIES_PER_MESH
    );
    assert_eq!(summary.omitted_identity_count(), 3);
    assert_eq!(summary.omitted_quad_count(), 3);
    assert!(
        summary
            .entries()
            .windows(2)
            .all(|pair| pair[0].sequential_id() < pair[1].sequential_id())
    );
}

#[test]
fn leaf_slab_is_cave_open_while_opaque_slab_separates_opposite_faces() {
    let leaf = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &slab(LEAF_A),
    );
    assert!(leaf.connectivity().is_all_connected());

    let opaque = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &slab(OPAQUE_A),
    );
    assert!(
        !opaque
            .connectivity()
            .is_connected(Face::NegativeX, Face::PositiveX)
    );
}

#[test]
fn separate_primary_layers_resolve_per_coordinate() {
    let layer_zero = packed_storage(1, &[AIR, LEAF_A], &[([1, 1, 1], 1)]);
    let layer_one = packed_storage(1, &[AIR, OPAQUE_A], &[([2, 1, 1], 1)]);
    let sub = sub_chunk(vec![layer_zero, layer_one]);
    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 11);
    assert!(!has_face(&mesh, [1, 1, 1], Face::PositiveX));
    assert!(has_face(&mesh, [2, 1, 1], Face::NegativeX));
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == LEAF_A)
            .count(),
        5
    );
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == OPAQUE_A)
            .count(),
        6
    );
}

#[test]
fn classifier_air_collision_with_known_opaque_visual_remains_air_in_mixed_storage() {
    let collision_classifier = BlockClassifier::new(OPAQUE_A);
    let layer_zero = packed_storage(1, &[OPAQUE_A, OPAQUE_B], &[([8, 8, 8], 1)]);
    let layer_one = packed_storage(1, &[OPAQUE_A, LEAF_A], &[([1, 1, 1], 1)]);
    let sub = sub_chunk(vec![layer_zero, layer_one]);
    let mesh = mesh(
        &collision_classifier,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 12);
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == LEAF_A)
            .count(),
        6
    );
    assert_eq!(
        mesh.quads()
            .iter()
            .filter(|quad| quad.material_id() == OPAQUE_B)
            .count(),
        6
    );
}

#[test]
fn classifier_non_air_collision_with_air_visual_stays_diagnostic_and_owns_the_voxel() {
    let collision_classifier = BlockClassifier::new(AIR - 1);
    let layer_zero = packed_storage(1, &[AIR - 1, AIR], &[([1, 1, 1], 1)]);
    let layer_one = packed_storage(1, &[AIR - 1, OPAQUE_A], &[([1, 1, 1], 1)]);
    let sub = sub_chunk(vec![layer_zero, layer_one]);
    let mesh = mesh(
        &collision_classifier,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert_eq!(mesh.quad_count(), 6);
    assert!(
        mesh.quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn configured_high_bit_air_is_empty_in_every_storage_layer() {
    const HASHED_AIR: u32 = 0xdbf4_4120;
    let classifier = BlockClassifier::new(HASHED_AIR);
    let sub = sub_chunk(vec![
        uniform_storage(HASHED_AIR),
        uniform_storage(HASHED_AIR),
    ]);

    let mesh = mesh(
        &classifier,
        NetworkIdMode::Hashed,
        &Neighbourhood::empty(),
        &sub,
    );

    assert!(mesh.is_empty());
    assert!(mesh.connectivity().is_all_connected());
}

#[test]
fn empty_tunnel_connects_only_the_two_faces_it_reaches() {
    let tunnel = (0..16).map(|x| ([x, 8, 8], 1)).collect::<Vec<_>>();
    let sub = sub_chunk(vec![packed_storage(1, &[43, AIR], &tunnel)]);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );
    let connectivity = mesh.connectivity();

    assert!(connectivity.is_connected(Face::NegativeX, Face::PositiveX));
    assert!(connectivity.is_connected(Face::PositiveX, Face::NegativeX));
    assert!(!connectivity.is_connected(Face::NegativeX, Face::NegativeY));
    assert!(!connectivity.is_connected(Face::PositiveX, Face::PositiveZ));
}

#[test]
fn sealed_empty_cavity_has_no_face_connectivity() {
    let sub = sub_chunk(vec![packed_storage(1, &[47, AIR], &[([8, 8, 8], 1)])]);

    let mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &sub,
    );

    assert!(mesh.connectivity().is_empty());
}

#[test]
fn explicit_network_mode_preserves_high_hashes_and_isolates_low_collisions() {
    let high_hash = runtime_assets().resolve(NetworkIdMode::Hashed, 0xdbf4_4120);
    assert!(high_hash.is_known());
    assert_eq!(high_hash.face(BlockFace::Up).material_id(), 64);

    let sequential = runtime_assets().resolve(NetworkIdMode::Sequential, 7);
    let colliding_hash = runtime_assets().resolve(NetworkIdMode::Hashed, 7);
    assert_eq!(sequential.face(BlockFace::West).material_id(), 7);
    assert_eq!(colliding_hash.face(BlockFace::West).material_id(), 66);

    let sub = blocks(7, &[[1, 2, 3]]);
    let hashed_mesh = mesh(
        &BlockClassifier::new(0xdbf4_4120),
        NetworkIdMode::Hashed,
        &Neighbourhood::empty(),
        &sub,
    );
    assert!(
        hashed_mesh
            .quads()
            .iter()
            .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL)
    );
}

#[test]
fn greedy_merge_identity_is_face_material_not_network_value() {
    let same_material = sub_chunk(vec![packed_storage(
        2,
        &[AIR, 51, 52],
        &[([0, 0, 0], 1), ([1, 0, 0], 2)],
    )]);
    let merged = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &same_material,
    );
    assert_eq!(merged.quad_count(), 6);
    assert!(merged.quads().iter().all(|quad| quad.material_id() == 51));

    let different_materials = sub_chunk(vec![packed_storage(
        2,
        &[AIR, 13, 17],
        &[([0, 0, 0], 1), ([1, 0, 0], 2)],
    )]);
    let split = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &different_materials,
    );
    assert_eq!(split.quad_count(), 10);
}

#[test]
fn exact_face_materials_and_diagnostic_fallback_are_packed() {
    let face_mapped = blocks(53, &[[4, 5, 6]]);
    let face_mesh = mesh(
        &classifier(),
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &face_mapped,
    );
    let expected = [61, 62, 63, 64, 65, 66];
    for face in Face::ALL {
        let quad = face_mesh
            .quads()
            .iter()
            .find(|quad| quad.face() == face)
            .expect("one quad per face");
        assert_eq!(quad.material_id(), expected[face as usize]);
    }

    for runtime_id in [54, 50_000] {
        let sub = blocks(runtime_id, &[[4, 5, 6]]);
        let mesh = mesh(
            &classifier(),
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &sub,
        );
        assert!(
            mesh.quads()
                .iter()
                .all(|quad| quad.material_id() == DIAGNOSTIC_MATERIAL),
            "runtime value {runtime_id} bypassed diagnostic material"
        );
    }
}
