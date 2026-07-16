struct CompiledBeeHousingFixture {
    assets: RuntimeAssets,
    air: NetworkValues,
    states: Vec<NetworkValues>,
}

fn write_bee_housing_render_pack(root: &Path) {
    fs::create_dir_all(root.join("textures/blocks")).expect("create bee fixture tree");
    fs::write(
        root.join("blocks.json"),
        r#"{
            "bee_nest":{"textures":{"down":"bee_nest_bottom","east":"bee_nest_side","north":"bee_nest_side","south":"bee_nest_front","up":"bee_nest_top","west":"bee_nest_side"}},
            "beehive":{"textures":{"down":"beehive_top","east":"beehive_side","north":"beehive_side","south":"beehive_front","up":"beehive_top","west":"beehive_side"}}
        }"#,
    )
    .expect("write bee block routes");
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{
            "bee_nest_bottom":{"textures":["textures/blocks/bee_nest_bottom"]},
            "bee_nest_front":{"textures":["textures/blocks/bee_nest_front","textures/blocks/bee_nest_front_honey"]},
            "bee_nest_side":{"textures":["textures/blocks/bee_nest_side"]},
            "bee_nest_top":{"textures":["textures/blocks/bee_nest_top"]},
            "beehive_front":{"textures":["textures/blocks/beehive_front","textures/blocks/beehive_front_honey"]},
            "beehive_side":{"textures":["textures/blocks/beehive_side"]},
            "beehive_top":{"textures":["textures/blocks/beehive_top"]}
        }}"#,
    )
    .expect("write bee terrain routes");
    fs::write(root.join("textures/flipbook_textures.json"), "[]")
        .expect("write bee empty flipbooks");
    for (index, name) in [
        "bee_nest_bottom",
        "bee_nest_front",
        "bee_nest_front_honey",
        "bee_nest_side",
        "bee_nest_top",
        "beehive_front",
        "beehive_front_honey",
        "beehive_side",
        "beehive_top",
    ]
    .into_iter()
    .enumerate()
    {
        let rgba = [20 + index as u8 * 12, 90, 140, 255].repeat(16 * 16);
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&rgba, 16, 16, ExtendedColorType::Rgba8)
            .expect("encode bee fixture PNG");
        fs::write(root.join(format!("textures/blocks/{name}.png")), png)
            .expect("write bee fixture PNG");
    }
}

fn compiled_bee_housing_fixture() -> &'static CompiledBeeHousingFixture {
    static FIXTURE: OnceLock<CompiledBeeHousingFixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let records = read_registry(include_bytes!("../../../assets/data/block-registry-v1001.bin"))
            .expect("decode bee registry");
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
                    "minecraft:bee_nest" | "minecraft:beehive"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 48);
        let values = |record: &assets::RegistryRecord| NetworkValues {
            sequential: record.sequential_id,
            hashed: record.network_hash,
        };
        let state_values = selected.iter().map(values).collect::<Vec<_>>();
        let air_values = values(&air);
        let directory = tempfile::tempdir().expect("bee fixture directory");
        write_bee_housing_render_pack(directory.path());
        let mut compile_records = Vec::with_capacity(49);
        compile_records.push(air);
        compile_records.extend(selected);
        let compiled =
            compile_pack(directory.path(), &compile_records).expect("compile bee fixture");
        assert!(compiled.model_templates.is_empty());
        assert!(compiled.model_quads.is_empty());
        let assets = RuntimeAssets::decode(
            &encode_blob(&compiled).expect("encode compiled bee render fixture"),
        )
        .expect("decode bee fixture");
        CompiledBeeHousingFixture {
            assets,
            air: air_values,
            states: state_values,
        }
    })
}

fn mesh_bee_housing(
    mode: NetworkIdMode,
    values: NetworkValues,
    coordinate: [u8; 3],
    neighbours: &Neighbourhood<'_>,
) -> ChunkMesh {
    let fixture = compiled_bee_housing_fixture();
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
fn every_bee_housing_state_uses_only_the_packed_cube_stream_in_both_network_modes() {
    let fixture = compiled_bee_housing_fixture();
    for &state in &fixture.states {
        let sequential = mesh_bee_housing(
            NetworkIdMode::Sequential,
            state,
            [7, 8, 9],
            &Neighbourhood::empty(),
        );
        let hashed = mesh_bee_housing(
            NetworkIdMode::Hashed,
            state,
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
fn bee_housing_culls_all_six_cross_subchunk_faces_in_both_network_modes() {
    let fixture = compiled_bee_housing_fixture();
    let state = fixture.states[23];
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let air = fixture.air.for_mode(mode);
        let block = state.for_mode(mode);
        for (face, center, neighbour) in [
            (Face::NegativeX, [0, 5, 6], [15, 5, 6]),
            (Face::PositiveX, [15, 5, 6], [0, 5, 6]),
            (Face::NegativeY, [5, 0, 6], [5, 15, 6]),
            (Face::PositiveY, [5, 15, 6], [5, 0, 6]),
            (Face::NegativeZ, [5, 6, 0], [5, 6, 15]),
            (Face::PositiveZ, [5, 6, 15], [5, 6, 0]),
        ] {
            let remote = sub_chunk(vec![packed_storage(1, &[air, block], &[(neighbour, 1)])]);
            let mesh = mesh_bee_housing(mode, state, center, &neighbourhood_for(face, &remote));
            assert_eq!(mesh.quad_count(), 5, "mode={mode:?} face={face:?}");
            assert!(!has_face(&mesh, center, face));
        }
    }
}

#[test]
fn dense_bee_housing_subchunks_greedy_mesh_to_six_quads_and_close_caves() {
    let fixture = compiled_bee_housing_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        for &state in &fixture.states {
            let center = sub_chunk(vec![uniform_storage(state.for_mode(mode))]);
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
            assert!(mesh.liquid_quads().is_empty());
            assert!(mesh.connectivity().is_empty());
        }
    }
}
