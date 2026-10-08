#[test]
fn honey_keeps_its_inset_core_and_full_bottom_textured_outer_shell() {
    let records = read_registry(include_bytes!(
        "../../../../assets/data/block-registry-v1001.bin"
    ))
    .expect("decode honey registry");
    let named = |name: &str| {
        records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .clone()
    };
    let air = named("minecraft:air");
    let honey = named("minecraft:honey_block");
    let ice = named("minecraft:ice");
    let stone = named("minecraft:stone");
    let directory = tempfile::tempdir().unwrap();
    write_stained_glass_render_pack(directory.path(), "honey_block");
    fs::write(directory.path().join("blocks.json"), r#"{
        "honey_block":{"textures":{"side":"cube","up":"red_stained_glass","down":"blue_stained_glass"}},
        "ice":{"textures":{"side":"cube","up":"red_stained_glass","down":"blue_stained_glass"}},
        "stone":{"textures":"cube"}
    }"#).unwrap();
    let ids = [air.sequential_id, honey.sequential_id, ice.sequential_id];
    let compiled = compile_pack(directory.path(), &[air, honey, ice, stone]).unwrap();
    let assets = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
    let honey_visual = assets.resolve(NetworkIdMode::Sequential, ids[1]);
    let head = honey_visual.model_template().unwrap();
    let parts = assets::model_template_parts(assets.model_templates(), head).unwrap();
    assert_eq!(parts.len(), 1);
    let quads = |part: &assets::ModelTemplate| {
        &assets.model_quads()[part.quad_start as usize..][..part.quad_count as usize]
    };
    assert_eq!(quads(&parts[0]).len(), 12);
    assert!(
        quads(&parts[0])[..6]
            .iter()
            .flat_map(|quad| quad.positions)
            .flatten()
            .all(|v| matches!(v, 16 | 240))
    );
    let bottom = honey_visual.face(assets::BlockFace::Down).material_id();
    assert!(
        quads(&parts[0])[6..]
            .iter()
            .all(|quad| quad.material == bottom)
    );
    assert!(
        quads(&parts[0])[6..]
            .iter()
            .flat_map(|quad| quad.positions)
            .flatten()
            .all(|v| matches!(v, 0 | 256))
    );
    assert_ne!(
        assets
            .resolve(NetworkIdMode::Sequential, ids[2])
            .model_template(),
        Some(head)
    );

    let mesh = |placements: &[([u8; 3], usize)]| {
        let center = sub_chunk(vec![packed_storage(2, &ids, placements)]);
        mesh_sub_chunk(
            &BlockClassifier::new(ids[0]),
            &assets,
            NetworkIdMode::Sequential,
            &Neighbourhood::empty(),
            &center,
        )
    };
    assert_eq!(
        mesh(&[([8, 8, 8], 1)]).transparent_model_draw_refs().len(),
        12
    );
    assert_eq!(
        mesh(&[([8, 8, 8], 2)]).transparent_model_draw_refs().len(),
        6
    );
    assert_eq!(
        mesh(&[([8, 8, 8], 1), ([9, 8, 8], 1)])
            .transparent_model_draw_refs()
            .len(),
        24
    );
}
