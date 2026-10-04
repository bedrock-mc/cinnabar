fn shade_network_id(mode: NetworkIdMode, id: u32) -> u32 {
    match mode {
        NetworkIdMode::Sequential => id,
        NetworkIdMode::Hashed => TEST_HASH_BASE + id,
    }
}

fn shade_test_chunk(mode: NetworkIdMode, placements: &[([u8; 3], u32)]) -> SubChunk {
    let palette = [AIR, SOLID, LEAF, EMITTING];
    let mut words = vec![0_u32; 256];
    for &([x, y, z], id) in placements {
        let index = palette
            .iter()
            .position(|&candidate| candidate == id)
            .unwrap();
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

const SHADE_TEST_FACE: [[i16; 3]; 4] = [[0, 256, 0], [0, 256, 256], [256, 256, 256], [256, 256, 0]];

#[test]
fn emitting_surface_keeps_its_directional_shading_in_direct_and_cached_bakes() {
    let assets = runtime_assets();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let classifier = BlockClassifier::new(shade_network_id(mode, AIR));
        let center = shade_test_chunk(mode, &[([8, 8, 8], EMITTING)]);
        let neighbourhood = MeshNeighbourhood::new(&center);
        let direct = bake_quad_lighting(
            &classifier, &assets, mode, &neighbourhood,
            [8, 8, 8], Face::PositiveY, SHADE_TEST_FACE,
        );
        assert!(direct.samples().iter().all(|sample| sample & (1 << 11) != 0));
        let mesh = meshing::mesh_sub_chunk_in_neighbourhood(
            &classifier, &assets, mode, &neighbourhood,
        );
        let index = mesh.cube_quads().iter().position(|quad| quad.face() == Face::PositiveY).unwrap();
        assert_eq!(mesh.cube_lighting()[index], direct);
    }
}

#[test]
fn leaf_shade_is_independent_of_diagonal_occlusion_in_direct_and_cached_bakes() {
    let assets = runtime_assets();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let classifier = BlockClassifier::new(shade_network_id(mode, AIR));
        let center = shade_test_chunk(
            mode,
            &[([8, 8, 8], SOLID), ([9, 9, 8], LEAF), ([8, 9, 9], LEAF)],
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
        // Vanilla leaf shade=.2; ambient occlusion still reads the
        // diagonal because neither leaf has the cached solid-render bit.
        assert_eq!((direct.samples()[2] >> 8) & 7, 2);
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
fn native_outward_leaf_and_emitter_shades_each_count_one_sample() {
    let assets = runtime_assets();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for shaded in [LEAF, EMITTING] {
            let classifier = BlockClassifier::new(shade_network_id(mode, AIR));
            let center = shade_test_chunk(mode, &[([8, 9, 8], shaded)]);
            let direct = bake_quad_lighting(
                &classifier,
                &assets,
                mode,
                &MeshNeighbourhood::new(&center),
                [8, 8, 8],
                Face::PositiveY,
                SHADE_TEST_FACE,
            );
            assert!(
                direct
                    .samples()
                    .into_iter()
                    .all(|sample| (sample >> 8) & 7 == 1)
            );
        }
    }
}

#[test]
fn leaf_in_additional_storage_does_not_change_primary_block_shade() {
    let assets = runtime_assets();
    let center = layered_uniform(&[AIR, LEAF]);
    let direct = bake_quad_lighting(
        &BlockClassifier::new(AIR),
        &assets,
        NetworkIdMode::Sequential,
        &MeshNeighbourhood::new(&center),
        [8, 8, 8],
        Face::PositiveY,
        SHADE_TEST_FACE,
    );
    assert!(
        direct
            .samples()
            .into_iter()
            .all(|sample| (sample >> 8) & 7 == 0)
    );
}
