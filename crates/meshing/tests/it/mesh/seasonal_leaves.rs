fn seasonal_leaf_fixture() -> RuntimeAssets {
    seasonal_leaf_fixture_with_metadata(0)
}

fn seasonal_leaf_fixture_with_metadata(metadata: u32) -> RuntimeAssets {
    let mut materials = vec![Material::unvaried(); 2];
    materials[1].flags = assets::MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_FOLIAGE_TINT;
    let base = materials.len() as u32;
    for (deep, exposed) in [(false, false), (false, true), (true, false), (true, true)] {
        for _face in Face::ALL {
            let mut material = materials[1];
            material.flags |= metadata;
            material.flags |= assets::MATERIAL_FLAG_SEASONAL_FOLIAGE
                | if exposed {
                    assets::MATERIAL_FLAG_EXPOSED_FOLIAGE
                } else {
                    0
                };
            if deep {
                material.flags &= !assets::MATERIAL_FLAG_ALPHA_CUTOUT;
            } else {
                material.flags |= assets::MATERIAL_FLAG_TWO_SIDED;
            }
            materials.push(material);
        }
    }
    let mut leaf = BlockVisual::diagnostic(
        BlockFlags::CUBE_GEOMETRY | BlockFlags::LEAF_MODEL,
        assets::ContributorRole::Primary,
    );
    leaf.kind = VisualKind::Cube;
    leaf.support = assets::VisualSupport::Exact;
    leaf.faces = [1; Face::ALL.len()];
    leaf.variant = assets::BLOCK_VISUAL_VARIANT_SEASONAL_LEAF | base;
    let mut nonseasonal_leaf = leaf;
    nonseasonal_leaf.variant =
        assets::BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF | materials.len() as u32;
    for deep in [false, true] {
        for _exposed in [false, true] {
            for _face in Face::ALL {
                let mut material = materials[1];
                material.flags = if deep {
                    0
                } else {
                    assets::MATERIAL_FLAG_ALPHA_CUTOUT | assets::MATERIAL_FLAG_TWO_SIDED
                };
                material.flags |= metadata;
                materials.push(material);
            }
        }
    }
    let mut stone = leaf;
    stone.flags = BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE;
    stone.variant = 0;
    let mut stone_material = Material::unvaried();
    stone_material.flags = metadata & assets::MATERIAL_FLAG_ISOTROPIC;
    stone.faces = [materials.len() as u32; Face::ALL.len()];
    materials.push(stone_material);
    let mut snow = stone;
    snow.flags = BlockFlags::CUBE_GEOMETRY;
    snow.variant = assets::BLOCK_VISUAL_VARIANT_TOP_SNOW;
    let mut thin_snow = snow;
    thin_snow.kind = VisualKind::Model;
    thin_snow.flags = BlockFlags::empty();
    thin_snow.model_template = 0;
    let mut replaceable_plant = thin_snow;
    replaceable_plant.kind = VisualKind::Cross;
    replaceable_plant.variant = 0;
    replaceable_plant.flags = BlockFlags::SEASONAL_REPLACEABLE;
    let mut flower = replaceable_plant;
    flower.flags = BlockFlags::empty();
    let mut water = replaceable_plant;
    water.kind = VisualKind::Liquid;
    water.model_template = assets::NO_MODEL_TEMPLATE;
    water.contributor_role = assets::ContributorRole::LiquidAdditional;
    let unknown = BlockVisual::diagnostic(BlockFlags::empty(), assets::ContributorRole::Primary);
    let mut air = BlockVisual::diagnostic(BlockFlags::AIR, assets::ContributorRole::Air);
    air.kind = VisualKind::Invisible;
    air.support = assets::VisualSupport::Exact;
    let visuals = vec![
        air,
        leaf,
        stone,
        snow,
        thin_snow,
        replaceable_plant,
        flower,
        water,
        unknown,
        nonseasonal_leaf,
    ]
    .into_boxed_slice();
    let compiled = CompiledAssets {
        light_properties: vec![assets::LightProperties::default(); visuals.len()]
            .into_boxed_slice(),
        hashed: (0..visuals.len() as u32)
            .map(|id| (0x10000 + id, id))
            .collect(),
        visuals,
        materials: materials.into_boxed_slice(),
        model_templates: vec![assets::ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: 0,
        }]
        .into_boxed_slice(),
        model_quads: vec![assets::ModelQuad {
            positions: [[0, 32, 0], [0, 32, 256], [256, 32, 256], [256, 32, 0]],
            uvs: [[0, 0], [0, 4096], [4096, 4096], [4096, 0]],
            material: 0,
            flags: 2,
        }]
        .into_boxed_slice(),
        animations: Box::new([]),
        animation_frames: Box::new([]),
        texture_pages: vec![TexturePage::new(TextureArray {
            layers: 1,
            mips: [16_u32, 8, 4, 2, 1]
                .into_iter()
                .map(|size| TextureMip {
                    size,
                    rgba8: vec![255; (size * size * 4) as usize].into_boxed_slice(),
                })
                .collect(),
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

fn seasonal_leaf_chunk(mode: NetworkIdMode, placements: &[([u8; 3], usize)]) -> SubChunk {
    seasonal_leaf_layered_chunk(mode, &[placements])
}

fn seasonal_leaf_layered_chunk(mode: NetworkIdMode, layers: &[&[([u8; 3], usize)]]) -> SubChunk {
    let palette = (0..10)
        .map(|id| {
            if mode == NetworkIdMode::Hashed {
                0x10000 + id
            } else {
                id
            }
        })
        .collect::<Vec<_>>();
    let mut data = vec![9, layers.len() as u8, 0];
    for placements in layers {
        data.extend(packed_storage(4, &palette, placements));
    }
    SubChunk::decode(&data, &RawBlockIds { air: palette[0] })
}

#[test]
fn native_leaf_faces_keep_block_local_uvs_and_clamped_edges_on_every_face() {
    for flags in [
        assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR,
        assets::MATERIAL_FLAG_NATIVE_LEAF_COLOUR | assets::MATERIAL_FLAG_ISOTROPIC,
    ] {
        let assets = seasonal_leaf_fixture_with_metadata(flags);
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let air = if mode == NetworkIdMode::Hashed {
                0x10000
            } else {
                0
            };
            let placements = (5..9)
                .flat_map(|x| (5..9).map(move |z| ([x, 8, z], 1)))
                .collect::<Vec<_>>();
            let chunk = seasonal_leaf_chunk(mode, &placements);
            let mesh = mesh_sub_chunk_in_neighbourhood(
                &BlockClassifier::new(air),
                &assets,
                mode,
                &MeshNeighbourhood::new(&chunk),
            );
            assert!(!mesh.cube_quads().is_empty());
            assert!(
                mesh.cube_quads()
                    .iter()
                    .all(|quad| quad.width() == 1 && quad.height() == 1)
            );
            for face in Face::ALL {
                assert!(mesh.cube_quads().iter().any(|quad| quad.face() == face));
            }
            // The ordinary carried leaf material must not inherit metadata.
            assert_eq!(assets.materials()[1].flags & flags, 0);
        }
    }
}

#[test]
fn isotropic_ordinary_cube_faces_keep_independent_positions_and_repeatable_meshes() {
    let placements = (5..9)
        .flat_map(|x| (5..9).map(move |z| ([x, 8, z], 2)))
        .collect::<Vec<_>>();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let air = if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        };
        let classifier = BlockClassifier::new(air);
        let chunk = seasonal_leaf_chunk(mode, &placements);
        let neighbourhood = MeshNeighbourhood::new(&chunk);
        for flags in [0, assets::MATERIAL_FLAG_ISOTROPIC] {
            let assets = seasonal_leaf_fixture_with_metadata(flags);
            let mesh = mesh_sub_chunk_in_neighbourhood(&classifier, &assets, mode, &neighbourhood);
            let repeated =
                mesh_sub_chunk_in_neighbourhood(&classifier, &assets, mode, &neighbourhood);
            assert_eq!(mesh.cube_quads(), repeated.cube_quads());
            let top = mesh
                .cube_quads()
                .iter()
                .filter(|quad| quad.face() == Face::PositiveY)
                .collect::<Vec<_>>();
            if flags != 0 {
                assert_eq!(top.len(), placements.len());
                assert!(
                    top.iter()
                        .all(|quad| quad.width() == 1 && quad.height() == 1)
                );
            } else {
                assert!(top.iter().any(|quad| quad.width() > 1 || quad.height() > 1));
            }
        }
    }
}

fn assert_seasonal_leaf_exposure(
    mesh: &ChunkMesh,
    assets: &RuntimeAssets,
    origin: [u8; 3],
    expected: bool,
) {
    let quads = mesh
        .cube_quads()
        .iter()
        .filter(|quad| quad.origin() == origin)
        .collect::<Vec<_>>();
    assert!(!quads.is_empty(), "leaf {origin:?} must produce geometry");
    assert!(
        quads.iter().all(
            |quad| (assets.materials()[quad.material_id() as usize].flags
                & assets::MATERIAL_FLAG_EXPOSED_FOLIAGE
                != 0)
                == expected
        ),
        "leaf {origin:?}: expected exposed={expected}"
    );
}

#[test]
fn seasonal_leaves_use_world_exposure_but_keep_carried_faces_ordinary() {
    let assets = seasonal_leaf_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let air = if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        };
        let center = seasonal_leaf_chunk(
            mode,
            &[
                ([7, 8, 7], 1),
                ([7, 10, 7], 1),
                ([8, 8, 7], 1),
                ([8, 9, 7], 4),
                ([9, 8, 7], 1),
                ([9, 9, 7], 3),
            ],
        );
        let roof = seasonal_leaf_chunk(mode, &[([7, 0, 7], 2)]);
        let mut neighbourhood = MeshNeighbourhood::new(&center);
        let exposed = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(air),
            &assets,
            mode,
            &neighbourhood,
        );
        assert_seasonal_leaf_exposure(&exposed, &assets, [7, 8, 7], true);
        assert_seasonal_leaf_exposure(&exposed, &assets, [8, 8, 7], true);
        assert_seasonal_leaf_exposure(&exposed, &assets, [9, 8, 7], false);
        assert!(neighbourhood.insert_column_above(3, &roof));
        let covered = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(air),
            &assets,
            mode,
            &neighbourhood,
        );
        assert_seasonal_leaf_exposure(&covered, &assets, [7, 8, 7], false);
        assert_seasonal_leaf_exposure(&covered, &assets, [8, 8, 7], true);
        let leaf = assets.resolve(
            mode,
            if mode == NetworkIdMode::Hashed {
                0x10001
            } else {
                1
            },
        );
        assert!(BlockFace::ALL.iter().all(|&face| {
            assets.materials()[leaf.face(face).material_id() as usize].flags
                & assets::MATERIAL_FLAG_SEASONAL_FOLIAGE
                == 0
        }));
        let dependencies =
            meshing::mesh_dependency_mask(&BlockClassifier::new(air), &assets, mode, &center);
        assert!(dependencies.seasonal_foliage);
        let stone_id = if mode == NetworkIdMode::Hashed {
            0x10002
        } else {
            2
        };
        let mut roof_bytes = vec![8, 1];
        roof_bytes.extend(uniform_storage(stone_id));
        let stone_only = SubChunk::decode(&roof_bytes, &RawBlockIds { air });
        assert!(
            !meshing::mesh_dependency_mask(&BlockClassifier::new(air), &assets, mode, &stone_only)
                .seasonal_foliage
        );
    }
}

#[test]
fn seasonal_leaves_observe_roof_add_remove_across_y15_y16() {
    let assets = seasonal_leaf_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let air = if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        };
        let classifier = BlockClassifier::new(air);
        let leaf = seasonal_leaf_chunk(mode, &[([7, 15, 7], 1)]);
        let roof = seasonal_leaf_chunk(mode, &[([7, 0, 7], 2)]);
        let mut neighbourhood = MeshNeighbourhood::new(&leaf);
        let exposed = mesh_sub_chunk_in_neighbourhood(&classifier, &assets, mode, &neighbourhood);
        assert_seasonal_leaf_exposure(&exposed, &assets, [7, 15, 7], true);
        assert!(neighbourhood.insert([0, 1, 0], &roof));
        let sheltered = mesh_sub_chunk_in_neighbourhood(&classifier, &assets, mode, &neighbourhood);
        assert_seasonal_leaf_exposure(&sheltered, &assets, [7, 15, 7], false);
        let removed = mesh_sub_chunk_in_neighbourhood(
            &classifier,
            &assets,
            mode,
            &MeshNeighbourhood::new(&leaf),
        );
        assert_seasonal_leaf_exposure(&removed, &assets, [7, 15, 7], true);
    }
}

#[test]
fn seasonal_shelter_uses_source_admission_and_raw_snow_extra_layer() {
    let assets = seasonal_leaf_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let air = if mode == NetworkIdMode::Hashed {
            0x10000
        } else {
            0
        };
        let main = [
            ([1, 4, 7], 1),
            ([1, 5, 7], 5),
            ([2, 4, 7], 1),
            ([2, 5, 7], 6),
            ([3, 4, 7], 1),
            ([3, 5, 7], 7),
            ([4, 4, 7], 1),
            ([4, 5, 7], 3),
            ([5, 4, 7], 1),
            ([5, 5, 7], 4),
            ([6, 4, 7], 1),
            ([6, 5, 7], 3),
            ([7, 4, 7], 1),
            ([7, 5, 7], 4),
            ([8, 4, 7], 1),
            ([8, 5, 7], 3),
            ([9, 4, 7], 1),
            ([9, 5, 7], 4),
            // Native ignores the extra layer when main is a leaves property-skip.
            ([10, 4, 7], 1),
            ([10, 5, 7], 1),
            // Storage order is not the rendering contributor's reordered surface.
            ([11, 4, 7], 1),
            ([11, 5, 7], 5),
        ];
        let extra = [
            ([4, 5, 7], 5),
            ([5, 5, 7], 6),
            ([6, 5, 7], 7),
            ([7, 5, 7], 8),
            ([8, 5, 7], 1),
            ([9, 5, 7], 6),
            ([10, 5, 7], 2),
            ([11, 5, 7], 3),
        ];
        let chunk = seasonal_leaf_layered_chunk(mode, &[&main, &extra]);
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(air),
            &assets,
            mode,
            &MeshNeighbourhood::new(&chunk),
        );
        for (x, exposed) in [
            (1, true),
            (2, false),
            (3, true),
            (4, true),
            (5, false),
            (6, true),
            (7, false),
            (8, false),
            (9, false),
            (10, true),
            (11, true),
        ] {
            assert_seasonal_leaf_exposure(&mesh, &assets, [x, 4, 7], exposed);
        }
    }
}

#[test]
fn seasonal_shelter_preserves_raw_layers_in_uniform_palette_fast_path() {
    let assets = seasonal_leaf_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let network_id = |id| {
            if mode == NetworkIdMode::Hashed {
                0x10000 + id
            } else {
                id
            }
        };
        let leaf = seasonal_leaf_chunk(mode, &[([7, 15, 7], 1)]);
        for (main, extra, exposed) in [
            (3, 0, false),
            (4, 0, true),
            (3, 5, true),
            (4, 6, false),
            (3, 7, true),
            (4, 8, false),
            (3, 1, false),
            (1, 2, true),
            (5, 3, true),
        ] {
            let mut data = vec![8, 2];
            data.extend(uniform_storage(network_id(main)));
            data.extend(uniform_storage(network_id(extra)));
            let roof = SubChunk::decode(&data, &RawBlockIds { air: network_id(0) });
            let mut neighbourhood = MeshNeighbourhood::new(&leaf);
            assert!(neighbourhood.insert([0, 1, 0], &roof));
            let mesh = mesh_sub_chunk_in_neighbourhood(
                &BlockClassifier::new(network_id(0)),
                &assets,
                mode,
                &neighbourhood,
            );
            assert_seasonal_leaf_exposure(&mesh, &assets, [7, 15, 7], exposed);
        }
    }
}
