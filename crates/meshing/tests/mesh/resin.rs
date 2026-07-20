struct CompiledResinClumpFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    resin: [NetworkValues; 64],
    cube: NetworkValues,
    water: NetworkValues,
}

fn write_resin_clump_render_pack(root: &Path, cube_name: &str) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create resin fixture tree");
    fs::write(
        root.join("blocks.json"),
        format!(
            r#"{{
                "resin_clump":{{"carried_textures":"resin_clump_carried","textures":"resin_clump"}},
                "water":{{"textures":"water"}},
                "{cube_name}":{{"textures":"cube"}}
            }}"#
        ),
    )
    .expect("write resin block routing");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "resin_clump":{"textures":"textures/blocks/resin_clump"},
            "water":{"textures":"textures/blocks/water"},
            "cube":{"textures":"textures/blocks/cube"}
        }}"#,
    )
    .expect("write resin terrain routing");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write resin empty flipbooks");
    for (index, name) in ["resin_clump", "water", "cube"].into_iter().enumerate() {
        let mut rgba = vec![0_u8; 16 * 16 * 4];
        for (pixel_index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
            let alpha = if name == "resin_clump" && pixel_index % 5 != 0 {
                0
            } else {
                255
            };
            pixel.copy_from_slice(&[30 + index as u8 * 80, 60, 100, alpha]);
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode resin fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write resin fixture PNG");
    }
}

fn compiled_resin_clump_fixture() -> &'static CompiledResinClumpFixture {
    static FIXTURE: OnceLock<CompiledResinClumpFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode resin registry");
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("air record")
            .clone();
        let mut resin = records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:resin_clump")
            .cloned()
            .collect::<Vec<_>>();
        resin.sort_unstable_by_key(|record| {
            record
                .model_state
                .get(ModelStateField::Connections)
                .expect("resin direction bits")
        });
        assert_eq!(resin.len(), 64);
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
        let resin_values = std::array::from_fn(|mask| {
            assert_eq!(
                resin[mask].model_state.get(ModelStateField::Connections),
                Some(mask as u32)
            );
            values(&resin[mask])
        });
        let air_values = values(&air);
        let cube_values = values(&cube);
        let water_values = values(&water);
        let cube_name = cube.name.strip_prefix("minecraft:").unwrap();
        let directory = tempfile::tempdir().expect("resin fixture directory");
        write_resin_clump_render_pack(directory.path(), cube_name);
        let mut selected = Vec::with_capacity(67);
        selected.push(air);
        selected.extend(resin);
        selected.push(cube);
        selected.push(water);
        let compiled = compile_pack(directory.path(), &selected).expect("compile resin fixture");
        let blob = encode_blob(&compiled).expect("encode resin fixture");
        let assets = RuntimeAssets::decode(&blob).expect("decode resin fixture");
        for (mask, values) in resin_values.iter().enumerate() {
            let visual = assets.resolve(NetworkIdMode::Sequential, values.sequential);
            assert_eq!(visual.kind(), VisualKind::Model, "mask {mask}");
            assert!(visual.flags().is_empty(), "mask {mask}");
            let template = assets.model_templates()[visual.model_template().unwrap() as usize];
            assert_eq!(
                template.quad_count,
                if mask == 0 {
                    6
                } else {
                    (mask as u32).count_ones()
                }
            );
        }
        CompiledResinClumpFixture {
            assets,
            air: air_values,
            resin: resin_values,
            cube: cube_values,
            water: water_values,
        }
    })
}

fn resin_sub_chunk(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    water: &[[u8; 3]],
) -> SubChunk {
    let fixture = compiled_resin_clump_fixture();
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

fn mesh_resin_clumps(
    mode: NetworkIdMode,
    placements: &[([u8; 3], NetworkValues)],
    water: &[[u8; 3]],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_resin_clump_fixture();
    let center = resin_sub_chunk(mode, placements, water);
    mesh_sub_chunk(
        &BlockClassifier::new(fixture.air.for_mode(mode)),
        &fixture.assets,
        mode,
        neighbours,
        &center,
    )
}

#[test]
fn compiled_resin_clumps_cover_all_masks_in_both_network_modes_with_stable_streams() {
    let fixture = compiled_resin_clump_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for (mask, &values) in fixture.resin.iter().enumerate() {
            let effective = if mask == 0 { 63 } else { mask as u32 };
            let quad_count = effective.count_ones();
            let resolved = fixture.assets.resolve(mode, values.for_mode(mode));
            let template = resolved.model_template().expect("resin template");
            let mesh =
                mesh_resin_clumps(mode, &[([7, 8, 9], values)], &[], &Neighbourhood::empty());
            assert!(mesh.cube_quads().is_empty(), "mode={mode:?} mask={mask}");
            assert_eq!(mesh.model_refs().len(), 1, "mode={mode:?} mask={mask}");
            assert_eq!(
                mesh.model_refs()[0].words(),
                [
                    7 | (8 << 4) | (9 << 8),
                    template,
                    0,
                    (1_u32 << quad_count) - 1,
                ],
                "mode={mode:?} mask={mask}"
            );
            assert_eq!(
                mesh.model_draw_refs().len(),
                quad_count as usize,
                "mode={mode:?} mask={mask}"
            );
            assert_eq!(
                mesh.model_lighting().len(),
                quad_count as usize,
                "mode={mode:?} mask={mask}"
            );
            assert!(mesh.transparent_model_draw_refs().is_empty());
            assert!(mesh.connectivity().is_all_connected(), "mask={mask}");
        }
    }
}

#[test]
fn compiled_resin_planes_survive_all_boundaries_and_do_not_occlude_supports_or_models() {
    let fixture = compiled_resin_clump_fixture();
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
            let mesh = mesh_resin_clumps(
                mode,
                &[(center, fixture.resin[63])],
                &[],
                &neighbourhood_for(face, &opaque),
            );
            assert_eq!(mesh.model_refs().len(), 1, "mode={mode:?} face={face:?}");
            assert_eq!(
                mesh.model_draw_refs().len(),
                6,
                "mode={mode:?} face={face:?}"
            );
            assert_eq!(
                mesh.model_lighting().len(),
                6,
                "mode={mode:?} face={face:?}"
            );
            assert_eq!(mesh.model_refs()[0].words()[3], 0x3f);
            assert!(mesh.connectivity().is_all_connected());
        }

        let support = resin_sub_chunk(
            mode,
            &[([8, 8, 8], fixture.cube), ([8, 8, 9], fixture.resin[16])],
            &[],
        );
        let support_mesh = mesh_sub_chunk(
            &BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &Neighbourhood::empty(),
            &support,
        );
        assert_eq!(support_mesh.cube_quads().len(), 6, "mode={mode:?}");
        assert_eq!(support_mesh.model_draw_refs().len(), 1, "mode={mode:?}");

        let adjacent = mesh_resin_clumps(
            mode,
            &[
                ([7, 8, 8], fixture.resin[63]),
                ([8, 8, 8], fixture.resin[63]),
            ],
            &[],
            &Neighbourhood::empty(),
        );
        assert_eq!(adjacent.model_refs().len(), 2);
        assert_eq!(adjacent.model_draw_refs().len(), 12);
        assert_eq!(adjacent.model_lighting().len(), 12);
    }
}

#[test]
fn compiled_resin_preserves_additional_water_contributor() {
    let fixture = compiled_resin_clump_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let center = resin_sub_chunk(mode, &[([8, 8, 8], fixture.resin[63])], &[[8, 8, 8]]);
        let resolved = ContributorResolver::new(
            BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &center,
        )
        .resolve([8, 8, 8]);
        assert_eq!(
            resolved.primary_network_value(),
            Some(fixture.resin[63].for_mode(mode))
        );
        assert_eq!(
            resolved.liquid_network_value(),
            Some(fixture.water.for_mode(mode))
        );
        assert_eq!(resolved.diagnostic_network_value(), None);
        let neighbourhood = MeshNeighbourhood::new(&center);
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &neighbourhood,
        );
        assert_eq!(mesh.model_refs().len(), 1);
        assert_eq!(mesh.model_draw_refs().len(), 6);
        assert_eq!(mesh.model_lighting().len(), 6);
        assert!(!mesh.liquid_quads().is_empty(), "mode={mode:?}");
        assert_eq!(mesh.liquid_quads().len(), mesh.liquid_lighting().len());
    }
}

#[test]
fn compiled_resin_dense_mask_63_has_exact_bounded_open_model_streams() {
    let fixture = compiled_resin_clump_fixture();
    let center = sub_chunk(vec![uniform_storage(fixture.resin[63].sequential)]);
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
