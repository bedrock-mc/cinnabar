struct CompiledSelectorAliasCubeFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    states: Vec<(u32, NetworkValues)>,
}

impl CompiledSelectorAliasCubeFixture {
    fn state(&self, original_id: u32) -> NetworkValues {
        self.states
            .iter()
            .find_map(|&(id, values)| (id == original_id).then_some(values))
            .unwrap_or_else(|| panic!("missing selector-alias state {original_id}"))
    }
}

fn write_selector_alias_cube_render_pack(root: &Path) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create selector-alias fixture tree");
    fs::write(
        root.join("blocks.json"),
        r#"{
            "bone_block":{"textures":{"down":"bone_block_top","side":"bone_block_side","up":"bone_block_top"}},
            "chiseled_quartz_block":{"textures":{"down":"chiseled_quartz_block_top","side":"chiseled_quartz_block_side","up":"chiseled_quartz_block_top"}},
            "hay_block":{"textures":{"down":"hayblock_top","side":"hayblock_side","up":"hayblock_top"}},
            "purpur_block":{"textures":"flattened_purpur_block"},
            "quartz_block":{"textures":{"down":"flattened_quartz_block_top","side":"flattened_quartz_block_side","up":"flattened_quartz_block_top"}},
            "smooth_quartz":{"textures":"smooth_quartz"},
            "tnt":{"textures":{"down":"flattened_tnt_bottom","side":"flattened_tnt_side","up":"flattened_tnt_top"}}
        }"#,
    )
    .expect("write selector-alias block routes");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "bone_block_top":{"textures":"textures/blocks/bone_block_top"},
            "bone_block_side":{"textures":"textures/blocks/bone_block_side"},
            "chiseled_quartz_block_top":{"textures":"textures/blocks/quartz_block_chiseled_top"},
            "chiseled_quartz_block_side":{"textures":"textures/blocks/quartz_block_chiseled"},
            "hayblock_top":{"textures":"textures/blocks/hay_block_top"},
            "hayblock_side":{"textures":"textures/blocks/hay_block_side"},
            "flattened_purpur_block":{"textures":"textures/blocks/purpur_block"},
            "flattened_quartz_block_top":{"textures":"textures/blocks/quartz_block_top"},
            "flattened_quartz_block_side":{"textures":"textures/blocks/quartz_block_side"},
            "smooth_quartz":{"textures":"textures/blocks/quartz_block_bottom"},
            "flattened_tnt_bottom":{"textures":"textures/blocks/tnt_bottom"},
            "flattened_tnt_side":{"textures":"textures/blocks/tnt_side"},
            "flattened_tnt_top":{"textures":"textures/blocks/tnt_top"}
        }}"#,
    )
    .expect("write selector-alias terrain routes");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write selector-alias flipbooks");
    for (index, name) in [
        "bone_block_top",
        "bone_block_side",
        "quartz_block_chiseled_top",
        "quartz_block_chiseled",
        "hay_block_top",
        "hay_block_side",
        "purpur_block",
        "quartz_block_top",
        "quartz_block_side",
        "quartz_block_bottom",
        "tnt_bottom",
        "tnt_side",
        "tnt_top",
    ]
    .into_iter()
    .enumerate()
    {
        let rgba = [10 + index as u8 * 7, 80, 130, 255].repeat(16 * 16);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode selector-alias fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write selector-alias fixture PNG");
    }
}

fn compiled_selector_alias_cube_fixture() -> &'static CompiledSelectorAliasCubeFixture {
    static FIXTURE: OnceLock<CompiledSelectorAliasCubeFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode selector-alias registry");
        let air = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:air")
            .expect("air record")
            .clone();
        let selected = records
            .iter()
            .filter(|record| {
                matches!(
                    record.name.as_ref(),
                    "minecraft:bone_block"
                        | "minecraft:chiseled_quartz_block"
                        | "minecraft:hay_block"
                        | "minecraft:purpur_block"
                        | "minecraft:quartz_block"
                        | "minecraft:smooth_quartz"
                        | "minecraft:tnt"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 38);
        let values = |record: &assets::RegistryRecord| NetworkValues {
            sequential: record.sequential_id,
            hashed: record.network_hash,
        };
        let state_values = selected
            .iter()
            .map(|record| (record.sequential_id, values(record)))
            .collect::<Vec<_>>();
        let air_values = values(&air);
        let directory = tempfile::tempdir().expect("selector-alias fixture directory");
        write_selector_alias_cube_render_pack(directory.path());
        let mut compile_records = Vec::with_capacity(39);
        compile_records.push(air);
        compile_records.extend(selected);
        let compiled = compile_pack(directory.path(), &compile_records)
            .expect("compile selector-alias fixture");
        assert!(compiled.model_templates.is_empty());
        assert!(compiled.model_quads.is_empty());
        assert!(compiled.animations.is_empty());
        let blob = encode_blob(&compiled).expect("encode selector-alias fixture");
        let assets = RuntimeAssets::decode(&blob).expect("decode selector-alias fixture");
        for &(_, values) in &state_values {
            for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
                let visual = assets.resolve(mode, values.for_mode(mode));
                assert!(visual.is_known());
                assert_eq!(visual.kind(), VisualKind::Cube);
                assert_eq!(
                    visual.flags(),
                    BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE
                );
                assert!(visual.model_template().is_none());
            }
        }
        CompiledSelectorAliasCubeFixture {
            assets,
            air: air_values,
            states: state_values,
        }
    })
}

fn mesh_selector_alias_cube(
    mode: NetworkIdMode,
    values: NetworkValues,
    coordinate: [u8; 3],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_selector_alias_cube_fixture();
    let air = fixture.air.for_mode(mode);
    let center = sub_chunk(vec![packed_storage(
        1,
        &[air, values.for_mode(mode)],
        &[(coordinate, 1)],
    )]);
    mesh_sub_chunk(
        &BlockClassifier::new(air),
        &fixture.assets,
        mode,
        neighbours,
        &center,
    )
}

#[test]
fn selector_alias_cubes_mesh_equivalently_in_both_network_modes_without_model_streams() {
    let fixture = compiled_selector_alias_cube_fixture();
    for &(_, values) in &fixture.states {
        let sequential = mesh_selector_alias_cube(
            NetworkIdMode::Sequential,
            values,
            [7, 8, 9],
            &Neighbourhood::empty(),
        );
        let hashed = mesh_selector_alias_cube(
            NetworkIdMode::Hashed,
            values,
            [7, 8, 9],
            &Neighbourhood::empty(),
        );
        assert_eq!(sequential.cube_quads(), hashed.cube_quads());
        assert_eq!(sequential.quad_count(), 6);
        assert!(sequential.model_refs().is_empty());
        assert!(sequential.model_draw_refs().is_empty());
        assert!(sequential.transparent_model_draw_refs().is_empty());
        assert!(sequential.liquid_quads().is_empty());
    }
}

#[test]
fn selector_alias_cube_culls_all_six_cross_subchunk_faces_in_both_network_modes() {
    let fixture = compiled_selector_alias_cube_fixture();
    let values = fixture.state(2_911);
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let air = fixture.air.for_mode(mode);
        let block = values.for_mode(mode);
        for (face, center, neighbour) in [
            (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
            (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
            (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
            (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
            (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
            (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
        ] {
            let remote = sub_chunk(vec![packed_storage(1, &[air, block], &[(neighbour, 1)])]);
            let mesh =
                mesh_selector_alias_cube(mode, values, center, &neighbourhood_for(face, &remote));
            assert_eq!(mesh.quad_count(), 5, "mode={mode:?} face={face:?}");
            assert!(!has_face(&mesh, center, face));
        }
    }
}

#[test]
fn dense_selector_alias_cube_subchunk_greedy_meshes_to_six_quads_and_closes_caves() {
    let fixture = compiled_selector_alias_cube_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let values = fixture.state(14_687).for_mode(mode);
        let center = sub_chunk(vec![uniform_storage(values)]);
        let mesh = mesh_sub_chunk(
            &BlockClassifier::new(fixture.air.for_mode(mode)),
            &fixture.assets,
            mode,
            &Neighbourhood::empty(),
            &center,
        );
        assert_eq!(mesh.quad_count(), 6);
        assert!(
            mesh.quads()
                .iter()
                .all(|quad| quad.width() == 16 && quad.height() == 16)
        );
        assert!(mesh.model_refs().is_empty());
        assert!(mesh.transparent_model_draw_refs().is_empty());
        assert!(mesh.liquid_quads().is_empty());
        assert!(mesh.connectivity().is_empty());
    }
}
