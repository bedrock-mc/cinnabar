struct CompiledCakeFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    cake: [NetworkValues; 7],
    cube: NetworkValues,
    water: NetworkValues,
}

fn write_cake_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create cake fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "cake":{{"textures":{{"down":"cake_bottom","east":"cake_side","north":"cake_side","south":"cake_side","up":"cake_top","west":"cake_west"}}}},
                "water":{{"textures":"water"}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write cake block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "cake_bottom":{"textures":["textures/blocks/cake_bottom","textures/blocks/cake_bottom"]},
            "cake_side":{"textures":["textures/blocks/cake_side","textures/blocks/cake_side"]},
            "cake_top":{"textures":["textures/blocks/cake_top","textures/blocks/cake_top"]},
            "cake_west":{"textures":["textures/blocks/cake_side","textures/blocks/cake_inner"]},
            "water":{"textures":"textures/blocks/water"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write cake terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write cake empty flipbooks");
    for (index, name) in [
        "cake_bottom",
        "cake_side",
        "cake_top",
        "cake_inner",
        "water",
        "cube",
    ]
    .into_iter()
    .enumerate()
    {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for (pixel_index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            let x = pixel_index % 16;
            let y = pixel_index / 16;
            let visible = match name {
                "cake_bottom" | "cake_top" => (1..=14).contains(&x) && (1..=14).contains(&y),
                "cake_side" | "cake_inner" => (1..=14).contains(&x) && y >= 8,
                _ => true,
            };
            let alpha = if visible { 255 } else { 0 };
            pixel.copy_from_slice(&[25 + index as u8 * 30, 70, 110, alpha]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode cake fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write cake fixture PNG");
    }
}

fn compiled_cake_fixture() -> &'static CompiledCakeFixture {
    static FIXTURE: OnceLock<CompiledCakeFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode cake registry");
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("air record")
            .clone();
        let mut cake = records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:cake")
            .cloned()
            .collect::<Vec<_>>();
        cake.sort_unstable_by_key(|record| {
            record
                .model_state
                .get(ModelStateField::Growth)
                .expect("cake bite")
        });
        assert_eq!(cake.len(), 7);
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
        let cake_values = std::array::from_fn(|bite| {
            assert_eq!(
                cake[bite].model_state.get(ModelStateField::Growth),
                Some(bite as u32)
            );
            assert_eq!(cake[bite].sequential_id, 14_055 + bite as u32);
            values(&cake[bite])
        });
        let air_values = values(&air);
        let cube_values = values(&cube);
        let water_values = values(&water);
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("cake fixture directory");
        write_cake_render_pack(directory.path(), cube_name);
        let mut selected = Vec::with_capacity(10);
        selected.push(air);
        selected.extend(cake);
        selected.push(cube);
        selected.push(water);
        let compiled = compile_pack(directory.path(), &selected).expect("compile cake fixture");
        let assets = RuntimeAssets::decode(&encode_blob(&compiled).expect("encode cake fixture"))
            .expect("decode cake fixture");
        for (bite, values) in cake_values.iter().enumerate() {
            for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
                let visual = assets.resolve(mode, values.for_mode(mode));
                assert_eq!(
                    visual.kind(),
                    VisualKind::Model,
                    "mode={mode:?} bite={bite}"
                );
                assert!(visual.flags().is_empty());
                assert_eq!(
                    assets.model_templates()[visual.model_template().unwrap() as usize].quad_count,
                    6
                );
            }
        }
        CompiledCakeFixture {
            assets,
            air: air_values,
            cake: cake_values,
            cube: cube_values,
            water: water_values,
        }
    })
}

fn cake_sub_chunk(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    water: &[[u8; 3]],
) -> SubChunk {
    let fixture = compiled_cake_fixture();
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
        _ => 3,
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

fn mesh_cakes(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    water: &[[u8; 3]],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_cake_fixture();
    let center = cake_sub_chunk(mode, placements, water);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air.for_mode(mode)),
        &fixture.assets,
        mode,
        neighbours,
        &center,
    )
}

#[test]
fn compiled_cake_bites_mesh_equivalently_by_sequential_id_and_hash() {
    let fixture = compiled_cake_fixture();
    for (bite, &values) in fixture.cake.iter().enumerate() {
        let mut witness = None;
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let resolved = fixture.assets.resolve(mode, values.for_mode(mode));
            let template = resolved.model_template().expect("cake template");
            let mesh = mesh_cakes(mode, &[([7, 8, 9], values)], &[], &Neighbourhood::empty());
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
                assert_eq!(&streams, expected, "mode={mode:?} bite={bite}");
            } else {
                witness = Some(streams);
            }
        }
    }
}

#[test]
fn compiled_cake_survives_boundaries_and_never_occludes_neighbours() {
    let fixture = compiled_cake_fixture();
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
            let mesh = mesh_cakes(
                mode,
                &[(center, fixture.cake[6])],
                &[],
                &neighbourhood_for(face, &opaque),
            );
            assert_eq!(mesh.model_refs().len(), 1);
            assert_eq!(mesh.model_refs()[0].words()[3], 0x3f);
            assert_eq!(mesh.model_draw_refs().len(), 6);
            assert_eq!(mesh.model_lighting().len(), 6);
            assert!(mesh.connectivity().is_all_connected());
        }
        let adjacent = cake_sub_chunk(
            mode,
            &[([8, 8, 8], fixture.cube), ([8, 8, 9], fixture.cake[0])],
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
fn compiled_cake_preserves_additional_water_contributor() {
    let fixture = compiled_cake_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let center = cake_sub_chunk(mode, &[([8, 8, 8], fixture.cake[6])], &[[8, 8, 8]]);
        let resolved = ContributorResolver::new(
            BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &center,
        )
        .resolve([8, 8, 8]);
        assert_eq!(
            resolved.primary_network_value(),
            Some(fixture.cake[6].for_mode(mode))
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
        assert_eq!(mesh.liquid_quads().len(), mesh.liquid_lighting().len());
    }
}

#[test]
fn compiled_cake_dense_subchunk_has_exact_bounded_open_model_streams() {
    let fixture = compiled_cake_fixture();
    let center = sub_chunk(vec![uniform_storage(fixture.cake[6].sequential)]);
    let mesh = mesh_sub_chunk(
        &BlockClassifier::new(fixture.air.sequential),
        &fixture.assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &center,
    );
    assert_eq!(mesh.model_refs().len(), 4096);
    assert_eq!(mesh.model_draw_refs().len(), 24_576);
    assert_eq!(mesh.model_lighting().len(), 24_576);
    assert!(
        mesh.model_refs()
            .iter()
            .enumerate()
            .all(|(index, reference)| reference.words()[2] == (index * 6) as u32)
    );
    assert!(mesh.transparent_model_draw_refs().is_empty());
    assert!(mesh.connectivity().is_all_connected());
}
