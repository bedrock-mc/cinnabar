use super::*;

#[test]
fn piston_inventory_uses_carried_faces_ahead_of_the_placed_fallback() {
    let target: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../../../assets/bedrock-target.json"
    ))
    .unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let registry =
        std::fs::read(repo.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let all = read_registry_for_protocol(&registry, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &std::fs::read(repo.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry,
        all.len(),
        protocol,
    )
    .unwrap();
    let records: Vec<_> = all
        .into_vec()
        .into_iter()
        .filter(|r| {
            r.traits.contains(BlockTraits::PISTON)
                || matches!(r.name.as_ref(), "minecraft:air" | "minecraft:stone")
        })
        .collect();
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("biomes")).unwrap();
    std::fs::write(
        root.path().join("biomes/plains.client_biome.json"),
        r#"{"minecraft:client_biome":{"description":{"identifier":"minecraft:plains"}}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("biomes/plains.biome.json"), r#"{"minecraft:biome":{"description":{"identifier":"minecraft:plains"},"components":{"minecraft:climate":{"temperature":0.5,"downfall":0.5}}}}"#).unwrap();
    std::fs::create_dir_all(root.path().join("textures/colormap")).unwrap();
    for map in assets::TintMapId::ALL {
        image::RgbImage::from_pixel(
            assets::TINT_MAP_SIZE,
            assets::TINT_MAP_SIZE,
            image::Rgb([100, 150, 100]),
        )
        .save(
            root.path()
                .join(format!("textures/colormap/{}.png", map.source_name())),
        )
        .unwrap();
    }

    super::tests::write_pack(
        root.path(),
        r#"{"stone":{"textures":"world"},"piston":{"textures":"world","carried_textures":{"up":"normal","down":"bottom","side":"side"}},"sticky_piston":{"textures":"world","carried_textures":{"up":"sticky","down":"bottom","side":"side"}}}"#,
        r#"{"texture_data":{"world":{"textures":"textures/blocks/world"},"normal":{"textures":"textures/blocks/normal"},"sticky":{"textures":"textures/blocks/sticky"},"bottom":{"textures":"textures/blocks/bottom"},"side":{"textures":"textures/blocks/side"}}}"#,
        "[]",
    );
    for (name, pixel) in [
        ("world", [0, 0, 255, 255]),
        ("normal", [255, 0, 0, 255]),
        ("sticky", [0, 255, 0, 255]),
        ("bottom", [100, 100, 100, 255]),
        ("side", [200, 200, 200, 255]),
    ] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(pixel))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let compiled = pack_compiler::compile_pack_with_biomes(
        root.path(),
        root.path(),
        &records,
        &[],
        &lights[..span],
        protocol,
    )
    .unwrap();
    let world = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut bodies = 0;
    for record in records
        .iter()
        .filter(|r| r.traits.contains(BlockTraits::PISTON))
    {
        let visual = BlockVisualId(record.sequential_id);
        if !record.traits.contains(BlockTraits::BODY_RELATIVE_FACING) {
            assert!(
                !blocks.is_carried_cube(&world, visual),
                "piston arms are not cubes"
            );
            continue;
        }
        assert!(
            blocks.is_carried_cube(&world, visual),
            "{} {}",
            record.name,
            record.canonical_state
        );
        let tiles = blocks
            .carried_cube_tiles(root.path(), visual)
            .unwrap()
            .unwrap();
        let expected = if record.name.as_ref() == "minecraft:sticky_piston" {
            [0, 255, 0, 255]
        } else {
            [255, 0, 0, 255]
        };
        assert_eq!(&tiles[BlockFace::Up as usize].rgba8[..4], &expected);
        bodies += 1;
    }
    assert!(bodies >= 12);
}
