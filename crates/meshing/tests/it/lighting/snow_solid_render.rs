fn snow_solid_test_chunk(mode: NetworkIdMode, placements: &[([u8; 3], u32)]) -> SubChunk {
    let palette = [AIR, SOLID, FULL_HEIGHT_SNOW];
    let mut words = vec![0_u32; 256];
    for &([x, y, z], id) in placements {
        let index = palette.iter().position(|&value| value == id).unwrap();
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        words[linear / 16] |= (index as u32) << ((linear % 16) * 2);
    }
    let mut bytes = vec![9, 1, 0, 5];
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend(zig_zag_i32(palette.len() as i32));
    for id in palette {
        bytes.extend(zig_zag_i32(shade_network_id(mode, id) as i32));
    }
    SubChunk::decode(
        &bytes,
        &RawBlockIds {
            air: shade_network_id(mode, AIR),
        },
    )
}

#[test]
fn native_full_height_snow_is_face_covering_but_not_solid_render() {
    let assets = runtime_assets();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let classifier = BlockClassifier::new(shade_network_id(mode, AIR));
        let visual = assets.resolve(mode, shade_network_id(mode, FULL_HEIGHT_SNOW));
        assert!(visual.flags().contains(BlockFlags::OCCLUDES_FULL_FACE));
        let center = snow_solid_test_chunk(
            mode,
            &[
                ([8, 8, 8], SOLID),
                ([9, 9, 8], FULL_HEIGHT_SNOW),
                ([8, 9, 9], FULL_HEIGHT_SNOW),
            ],
        );
        let neighbourhood = MeshNeighbourhood::new(&center);
        let sampler = |p| MeshLightSample::try_new(if p == [9, 9, 9] { 15 } else { 2 }, 0).unwrap();
        let direct = bake_quad_lighting_with_sampler(
            &classifier,
            &assets,
            mode,
            &neighbourhood,
            &sampler,
            [8, 8, 8],
            Face::PositiveY,
            SHADE_TEST_FACE,
        );
        assert_eq!((direct.samples()[2] >> 8) & 7, 0);
        assert_eq!(direct.samples()[2] & 15, 15);
        let mesh = meshing::mesh_sub_chunk_in_neighbourhood_with_lighting(
            &classifier,
            &assets,
            mode,
            &neighbourhood,
            &sampler,
        );
        let index = mesh
            .cube_quads()
            .iter()
            .position(|quad| quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveY)
            .expect("unoccluded top face");
        assert_eq!(mesh.cube_lighting()[index], direct, "{mode:?}");
    }
}

#[test]
fn native_full_height_snow_center_light_uses_own_cell_in_direct_and_cached_bakes() {
    let assets = runtime_assets();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let classifier = BlockClassifier::new(shade_network_id(mode, AIR));
        let center = snow_solid_test_chunk(mode, &[([8, 8, 8], FULL_HEIGHT_SNOW)]);
        let neighbourhood = MeshNeighbourhood::new(&center);
        let sampler = |p| MeshLightSample::try_new(if p == [8, 8, 8] { 7 } else { 2 }, 0).unwrap();
        let direct = bake_quad_lighting_with_sampler(
            &classifier,
            &assets,
            mode,
            &neighbourhood,
            &sampler,
            [8, 8, 8],
            Face::PositiveY,
            SHADE_TEST_FACE,
        );
        assert_eq!(direct.samples(), [7; 4]);
        let mesh = meshing::mesh_sub_chunk_in_neighbourhood_with_lighting(
            &classifier,
            &assets,
            mode,
            &neighbourhood,
            &sampler,
        );
        let index = mesh
            .cube_quads()
            .iter()
            .position(|quad| quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveY)
            .expect("full-height snow top face");
        assert_eq!(mesh.cube_lighting()[index], direct, "{mode:?}");
    }
}

#[test]
fn native_full_height_snow_still_culls_covered_geometry_faces() {
    let assets = runtime_assets();
    let center = snow_solid_test_chunk(
        NetworkIdMode::Sequential,
        &[([8, 8, 8], FULL_HEIGHT_SNOW), ([9, 8, 8], FULL_HEIGHT_SNOW)],
    );
    let mesh = meshing::mesh_sub_chunk_in_neighbourhood_with_lighting(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&center),
        &meshing::FullBrightLightSampler,
    );
    // Greedy meshing joins the ten exposed unit faces into a six-face cuboid.
    assert_eq!(mesh.cube_quads().len(), 6);
    assert!(!mesh.cube_quads().iter().any(|quad| {
        (quad.origin() == [8, 8, 8] && quad.face() == Face::PositiveX)
            || (quad.origin() == [9, 8, 8] && quad.face() == Face::NegativeX)
    }));
}
