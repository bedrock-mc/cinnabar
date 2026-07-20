struct CompiledFarmlandFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    farmland: [NetworkValues; 8],
    cube: NetworkValues,
    water: NetworkValues,
}

fn write_farmland_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create farmland fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "farmland":{{"textures":{{"down":"farmland_side","side":"farmland_side","up":"farmland"}}}},
                "water":{{"textures":"water"}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write farmland block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "farmland_side":{"textures":"textures/blocks/dirt"},
            "farmland":{"textures":["textures/blocks/farmland_wet","textures/blocks/farmland_dry"]},
            "water":{"textures":"textures/blocks/water"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write farmland terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write farmland empty flipbooks");
    for (index, name) in ["dirt", "farmland_wet", "farmland_dry", "water", "cube"]
        .into_iter()
        .enumerate()
    {
        let rgba = vec![25 + index as u8 * 30, 70, 110, 255]
            .into_iter()
            .cycle()
            .take(16 * 16 * 4)
            .collect::<Vec<_>>();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode farmland fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write farmland fixture PNG");
    }
}

fn compiled_farmland_fixture() -> &'static CompiledFarmlandFixture {
    static FIXTURE: OnceLock<CompiledFarmlandFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode farmland registry");
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("air record")
            .clone();
        let mut farmland = records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:farmland")
            .cloned()
            .collect::<Vec<_>>();
        farmland.sort_unstable_by_key(|record| {
            record
                .model_state
                .get(ModelStateField::Growth)
                .expect("farmland moisture")
        });
        assert_eq!(farmland.len(), 8);
        let cube = records
            .iter()
            .find(|record| {
                record.name.as_ref() == "minecraft:stone"
                    && record.model_family == ModelFamily::Cube
                    && record.flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
            })
            .expect("stone full cube")
            .clone();
        let water = records
            .iter()
            .find(|record| {
                record.name.as_ref() == "minecraft:water"
                    && record.model_state.get(ModelStateField::LiquidDepth) == Some(0)
            })
            .expect("water depth-zero record")
            .clone();
        let values = |record: &assets::RegistryRecord| NetworkValues {
            sequential: record.sequential_id,
            hashed: record.network_hash,
        };
        let farmland_values = std::array::from_fn(|amount| {
            assert_eq!(
                farmland[amount].model_state.get(ModelStateField::Growth),
                Some(amount as u32)
            );
            assert_eq!(farmland[amount].sequential_id, 6_122 + amount as u32);
            values(&farmland[amount])
        });
        let air_values = values(&air);
        let cube_values = values(&cube);
        let water_values = values(&water);
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("farmland fixture directory");
        write_farmland_render_pack(directory.path(), cube_name);
        let mut selected = Vec::with_capacity(11);
        selected.push(air);
        selected.extend(farmland);
        selected.push(cube);
        selected.push(water);
        let compiled = compile_pack(directory.path(), &selected).expect("compile farmland fixture");
        let assets =
            RuntimeAssets::decode(&encode_blob(&compiled).expect("encode farmland fixture"))
                .expect("decode farmland fixture");
        for (amount, values) in farmland_values.iter().enumerate() {
            for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
                let visual = assets.resolve(mode, values.for_mode(mode));
                assert_eq!(
                    visual.kind(),
                    VisualKind::Model,
                    "mode={mode:?} amount={amount}"
                );
                assert!(visual.flags().is_empty());
                assert_eq!(
                    assets.model_templates()[visual.model_template().unwrap() as usize].quad_count,
                    6
                );
            }
        }
        CompiledFarmlandFixture {
            assets,
            air: air_values,
            farmland: farmland_values,
            cube: cube_values,
            water: water_values,
        }
    })
}

fn farmland_sub_chunk(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    water: &[[u8; 3]],
) -> SubChunk {
    let fixture = compiled_farmland_fixture();
    let air = fixture.air.for_mode(mode);
    let mut palette = vec![air];
    let primary = placements
        .iter()
        .map(|&(coordinate, values)| {
            let value = values.for_mode(mode);
            let index = palette
                .iter()
                .position(|&candidate| candidate == value)
                .unwrap_or_else(|| {
                    palette.push(value);
                    palette.len() - 1
                });
            (coordinate, index)
        })
        .collect::<Vec<_>>();
    let bits = match palette.len() {
        0..=2 => 1,
        3..=4 => 2,
        _ => 4,
    };
    let mut storages = vec![packed_storage(bits, &palette, &primary)];
    if !water.is_empty() {
        let water_placements = water
            .iter()
            .copied()
            .map(|coordinate| (coordinate, 1))
            .collect::<Vec<_>>();
        storages.push(packed_storage(
            1,
            &[air, fixture.water.for_mode(mode)],
            &water_placements,
        ));
    }
    sub_chunk(storages)
}

fn mesh_farmland(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    water: &[[u8; 3]],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_farmland_fixture();
    let center = farmland_sub_chunk(mode, placements, water);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air.for_mode(mode)),
        &fixture.assets,
        mode,
        neighbours,
        &center,
    )
}

#[test]
fn compiled_farmland_meshes_equivalently_by_sequential_id_and_hash() {
    let fixture = compiled_farmland_fixture();
    for (amount, &values) in fixture.farmland.iter().enumerate() {
        let mut witness = None;
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let resolved = fixture.assets.resolve(mode, values.for_mode(mode));
            let template = resolved.model_template().expect("farmland template");
            let mesh = mesh_farmland(mode, &[([7, 8, 9], values)], &[], &Neighbourhood::empty());
            assert!(mesh.cube_quads().is_empty());
            assert_eq!(mesh.model_refs().len(), 1);
            assert_eq!(
                mesh.model_refs()[0].words(),
                [7 | (8 << 4) | (9 << 8), template, 0, 0x3f]
            );
            assert_eq!(
                mesh.model_draw_refs()
                    .iter()
                    .copied()
                    .map(PackedModelDrawRef::words)
                    .collect::<Vec<_>>(),
                [[0, 0], [0, 1], [0, 2], [0, 3], [0, 4], [0, 5]]
            );
            assert_eq!(mesh.model_lighting().len(), 6);
            assert!(mesh.transparent_model_draw_refs().is_empty());
            assert!(mesh.connectivity().is_all_connected());
            let streams = (
                mesh.model_refs().to_vec(),
                mesh.model_draw_refs().to_vec(),
                mesh.model_lighting().to_vec(),
            );
            if let Some(expected) = &witness {
                assert_eq!(&streams, expected, "mode={mode:?} amount={amount}");
            } else {
                witness = Some(streams);
            }
        }
    }
}

#[test]
fn compiled_farmland_survives_boundaries_and_never_occludes_neighbours() {
    let fixture = compiled_farmland_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for (face, center, neighbour) in [
            (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
            (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
            (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
            (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
            (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
            (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
        ] {
            let opaque = sub_chunk(vec![packed_storage(
                1,
                &[fixture.air.for_mode(mode), fixture.cube.for_mode(mode)],
                &[(neighbour, 1)],
            )]);
            let mesh = mesh_farmland(
                mode,
                &[(center, fixture.farmland[7])],
                &[],
                &neighbourhood_for(face, &opaque),
            );
            assert_eq!(mesh.model_refs().len(), 1);
            assert_eq!(mesh.model_draw_refs().len(), 6);
            assert_eq!(mesh.model_lighting().len(), 6);
            assert!(mesh.connectivity().is_all_connected());
        }
        let adjacent = farmland_sub_chunk(
            mode,
            &[([8, 8, 8], fixture.cube), ([8, 8, 9], fixture.farmland[0])],
            &[],
        );
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &Neighbourhood::empty(),
            &adjacent,
        );
        assert_eq!(mesh.cube_quads().len(), 6);
        assert_eq!(mesh.model_draw_refs().len(), 6);
    }
}

#[test]
fn compiled_farmland_preserves_additional_water_contributor() {
    let fixture = compiled_farmland_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let center = farmland_sub_chunk(mode, &[([8, 8, 8], fixture.farmland[7])], &[[8, 8, 8]]);
        let resolved = ContributorResolver::new(
            BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &center,
        )
        .resolve([8, 8, 8]);
        assert_eq!(
            resolved.primary_network_value(),
            Some(fixture.farmland[7].for_mode(mode))
        );
        assert_eq!(
            resolved.liquid_network_value(),
            Some(fixture.water.for_mode(mode))
        );
        assert_eq!(resolved.diagnostic_network_value(), None);
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &MeshNeighbourhood::new(&center),
        );
        assert_eq!(mesh.model_refs().len(), 1);
        assert_eq!(mesh.model_draw_refs().len(), 6);
        assert_eq!(mesh.model_lighting().len(), 6);
        assert!(!mesh.liquid_quads().is_empty());
    }
}

#[test]
fn compiled_farmland_dense_uniform_and_mixed_products_are_bounded() {
    let fixture = compiled_farmland_fixture();
    let uniform = sub_chunk(vec![uniform_storage(fixture.farmland[7].sequential)]);
    let mut placements = Vec::with_capacity(4096);
    for y in 0..16_u8 {
        for z in 0..16_u8 {
            for x in 0..16_u8 {
                placements.push(([x, y, z], ((x as usize + y as usize + z as usize) % 8) + 1));
            }
        }
    }
    let palette = std::iter::once(fixture.air.sequential)
        .chain(fixture.farmland.iter().map(|value| value.sequential))
        .collect::<Vec<_>>();
    let mixed = sub_chunk(vec![packed_storage(4, &palette, &placements)]);
    for center in [&uniform, &mixed] {
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(fixture.air.sequential),
            &fixture.assets,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            center,
        );
        assert_eq!(mesh.model_refs().len(), 4096);
        assert_eq!(mesh.model_draw_refs().len(), 24_576);
        assert_eq!(mesh.model_lighting().len(), 24_576);
        assert!(mesh.cube_quads().is_empty());
        assert!(mesh.transparent_model_draw_refs().is_empty());
        assert!(mesh.connectivity().is_all_connected());
    }
}
