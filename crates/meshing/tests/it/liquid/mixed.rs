//! Mixed-neighbourhood golden meshes and transparent-cube flow contacts.

use super::*;

/// Deterministic dense 3x3x3 fixture shared by the mesh-output golden digests.
fn mixed_neighbourhood_chunks(seed: u64, density: u64) -> Vec<SubChunk> {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let ids = [
        SOLID,
        GLASS,
        CROSS,
        WATER_SOURCE,
        WATER_SOURCE + 3,
        WATER_SOURCE + 9,
        WATER_ALIAS,
        OTHER_LIQUID,
        FACED_LIQUID,
        FACED_DEPTH_7,
        NON_WATER_LIQUID,
    ];
    (0..27)
        .map(|_| {
            let mut primary = Vec::new();
            let mut waterlogged = Vec::new();
            for x in 0..16_u8 {
                for y in 0..16_u8 {
                    for z in 0..16_u8 {
                        if next() % 100 >= density {
                            continue;
                        }
                        let id = ids[(next() % ids.len() as u64) as usize];
                        primary.push((id, [x, y, z]));
                        if id == CROSS && next() % 2 == 0 {
                            waterlogged.push(([x, y, z], 1));
                        }
                    }
                }
            }
            let mut palette = vec![AIR];
            let placements = primary
                .iter()
                .map(|&(id, pos)| {
                    let index = palette.iter().position(|&v| v == id).unwrap_or_else(|| {
                        palette.push(id);
                        palette.len() - 1
                    });
                    (pos, index)
                })
                .collect::<Vec<_>>();
            sub_chunk(vec![
                packed_storage(5, &palette, &placements),
                packed_storage(1, &[AIR, WATER_SOURCE], &waterlogged),
            ])
        })
        .collect()
}

fn mesh_mixed(chunks: &[SubChunk]) -> meshing::ChunkMesh {
    let mut neighbourhood = MeshNeighbourhood::new(&chunks[13]);
    let mut index = 0;
    for x in -1..=1_i8 {
        for y in -1..=1_i8 {
            for z in -1..=1_i8 {
                if [x, y, z] != [0, 0, 0] {
                    assert!(neighbourhood.insert([x, y, z], &chunks[index]));
                }
                index += 1;
            }
        }
    }
    let sampler = |[x, y, z]: [i32; 3]| {
        let hash = (x.wrapping_mul(73_856_093)
            ^ y.wrapping_mul(19_349_663)
            ^ z.wrapping_mul(83_492_791)) as u32;
        MeshLightSample::try_new((hash & 15) as u8, ((hash >> 4) & 15) as u8).unwrap()
    };
    mesh_sub_chunk_in_neighbourhood_with_lighting(
        &BlockClassifier::new(AIR),
        runtime_assets(),
        NetworkIdMode::Sequential,
        &neighbourhood,
        &sampler,
    )
}

/// Cube-shaped transparent barriers are excluded from the native height sample, and their motion-blocking material stops downhill flow. Face transparency must not turn glass into an air sample.
#[test]
fn transparent_cube_over_water_is_not_an_open_flow_neighbour() {
    let mut placements = Vec::new();
    for x in 7..=9 {
        for z in 7..=9 {
            placements.push((
                if [x, z] == [9, 8] {
                    GLASS
                } else {
                    WATER_SOURCE
                },
                [x, 8, z],
            ));
        }
    }
    placements.push((WATER_SOURCE, [9, 7, 8]));
    let mesh = mesh(&blocks(&placements));
    let top = quad_at(&mesh, [8, 8, 8], Face::PositiveY);
    assert_eq!(top.flow_gradient(), [0, 0]);
    assert_eq!(
        top.heights(),
        [LiquidLevel::from_variant(0).unwrap().height(); 4]
    );
    assert_eq!(top.material_id(), STILL);
}

/// Dense mixed scenes retain extra-air contacts, primary-air winding, flow barriers,
/// liquid inset flags and no-AO lighting.
#[test]
fn mixed_neighbourhood_mesh_output_is_golden() {
    let digests = [(1_u64, 8_u64), (2, 30), (3, 70), (4, 95)].map(|(seed, density)| {
        let chunks = mixed_neighbourhood_chunks(seed, density);
        let mesh = mesh_mixed(&chunks);
        liquid_contacts::assert_thin_primary_contacts(&chunks, &mesh);
        assert!(!mesh.cube_quads().is_empty() && !mesh.liquid_quads().is_empty());
        format!("{mesh:?}")
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            })
    });
    assert_eq!(
        digests,
        [
            12_126_709_224_666_265_908,
            985_666_959_304_608_936,
            15_418_509_414_702_196_572,
            14_508_846_418_541_417_581
        ]
    );
}

/// Layer conflicts must resolve to the same diagnostic/primary/liquid winners.
/// Diagnostic terrain cubes with non-solid source winners use their own center
/// light; liquids use native inset flags and no-AO lighting.
#[test]
fn conflicting_layer_mesh_output_is_golden() {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let ids = [
        AIR,
        SOLID,
        CROSS,
        WATER_SOURCE,
        WATER_SOURCE + 2,
        OTHER_LIQUID,
    ];
    let chunks = (0..27)
        .map(|_| {
            let layers = (0..3)
                .map(|_| {
                    let placements = (0..4096_u32)
                        .filter_map(|i| {
                            let id = ids[(next() % ids.len() as u64) as usize];
                            let position = [(i >> 8) as u8, (i & 15) as u8, ((i >> 4) & 15) as u8];
                            (id != AIR).then_some((position, ids.iter().position(|&v| v == id)?))
                        })
                        .collect::<Vec<_>>();
                    packed_storage(3, &ids, &placements)
                })
                .collect();
            sub_chunk(layers)
        })
        .collect::<Vec<_>>();
    let mesh = mesh_mixed(&chunks);
    liquid_contacts::assert_thin_primary_contacts(&chunks, &mesh);
    let digest = format!("{mesh:?}")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    assert!(!mesh.cube_quads().is_empty());
    assert_eq!(digest, 5_225_287_854_792_396_798);
}
