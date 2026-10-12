/// Compiles actual column identities against original scalar replacement sprites.
fn bubble_column_fixture() -> (RuntimeAssets, [RegistryRecord; 5]) {
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let records = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        target["wire_protocol"].as_u64().unwrap() as u32,
    )
    .unwrap();
    let selected = ["air", "stone", "water", "bubble_column", "bubble_column"].map(|name| {
        records
            .iter()
            .find(|r| r.name.as_ref() == format!("minecraft:{name}"))
            .unwrap()
            .clone()
    });
    let mut selected = selected;
    selected[4] = records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:bubble_column")
        .nth(1)
        .unwrap()
        .clone();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("textures")).unwrap();
    fs::write(directory.path().join("blocks.json"), r#"{
        "bubble_column":{"textures":{"up":"bubble","down":"bubble","north":"bubble","south":"bubble","west":"pumpkin","east":"pumpkin"}},
        "water":{"textures":"water"},"stone":{"textures":"pumpkin"}}
    "#).unwrap();
    fs::write(directory.path().join("textures/terrain_texture.json"), r#"{"texture_data":{
        "bubble":{"textures":"textures/bubble"},"water":{"textures":"textures/water"},"pumpkin":{"textures":"textures/pumpkin"}}}"#).unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .unwrap();
    for (name, color) in [
        ("bubble", [180, 220, 240, 100]),
        ("water", [40, 80, 180, 160]),
        ("pumpkin", [255, 80, 0, 255]),
    ] {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(
                &color.repeat((assets::TILE_SIZE * assets::TILE_SIZE) as usize),
                assets::TILE_SIZE,
                assets::TILE_SIZE,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        fs::write(directory.path().join(format!("textures/{name}.png")), png).unwrap();
    }
    let compiled = compile_pack(directory.path(), &selected).unwrap();
    (
        RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap(),
        selected,
    )
}

#[test]
fn bubble_column_mesh_retains_water_and_selects_air_only_surfaces_across_boundaries() {
    let (runtime, records) = bubble_column_fixture();
    for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
        let ids = records.each_ref().map(|r| match mode {
            NetworkIdMode::Sequential => r.sequential_id,
            NetworkIdMode::Hashed => r.network_hash,
        });
        for column in [3, 4] {
            for above in [0, 1, 2, 3, 4] {
                for y in [8, 15] {
                    let placements = [([7, y, 9], column), ([7, y + 1, 9], above)];
                    let water = [([7, y, 9], 1), ([7, y + 1, 9], 1)];
                    let center = sub_chunk(vec![
                        packed_storage(
                            3,
                            &ids,
                            &placements[..if y == 15 { 1 } else { 2 }],
                        ),
                        packed_storage(1, &[ids[0], ids[2]], &water[..if y == 15 { 1 } else { 2 }]),
                    ]);
                    let upper = sub_chunk(vec![uniform_storage(ids[above]), uniform_storage(ids[2])]);
                    let mut neighbourhood =
                        MeshNeighbourhood::new(&center).with_block_origin([256, -32, 0]);
                    assert!(neighbourhood.insert([0, 1, 0], &upper));
                    let mesh = mesh_sub_chunk_in_neighbourhood(
                        &BlockClassifier::new(ids[0]),
                        &runtime,
                        mode,
                        &neighbourhood,
                    );
                    assert!(
                        !mesh.liquid_quads().is_empty(),
                        "column must retain extra water"
                    );
                    assert!(mesh.diagnostic_geometry().entries().is_empty());
                    let base = runtime.resolve(mode, ids[column]).model_template().unwrap();
                    let reference = mesh
                        .model_refs()
                        .iter()
                        .find(|r| r.words()[0] & 0xfff == (7 | (u32::from(y) << 4) | (9 << 8)))
                        .unwrap();
                    let expected_count = if above == 0 { 9 } else { 8 };
                    let template = runtime.model_templates()[reference.words()[1] as usize];
                    assert_eq!(
                        template.quad_count, expected_count,
                        "column={column},above={above},y={y}"
                    );
                    assert_eq!(
                        (reference.words()[1] - base) % assets::BUBBLE_COLUMN_POSITION_VARIANTS,
                        1
                    );
                    assert_eq!(reference.words()[3].count_ones(), expected_count);
                    assert!(
                        meshing::mesh_output_byte_len(
                            &mesh,
                            &meshing::PackedBiomeRecord::fallback()
                        ) <= meshing::MeshOutputBounds::new(&runtime)
                            .for_sub_chunk(&center, &runtime, mode)
                    );
                }
            }
        }
    }
}
