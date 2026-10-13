use std::{fs, path::Path};

use assets::{BlockVisualId, CompiledBiomeAssets, RuntimeAssets, RuntimeIconCatalog};
use pack_compiler::compile_pack_with_biomes;
use pack_entity_compiler::{compile_entity_assets, compile_icon_assets_with_blocks};
use sha2::{Digest, Sha256};

/// Writes one source beneath the fixture root.
fn write(root: &Path, path: &str, bytes: &[u8]) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Compiles barrel world geometry through the terrain owner's public API.
fn barrel_world(root: &Path) -> (assets::CompiledAssets, BlockVisualId) {
    let registry_bytes = assets::pinned_block_registry_bytes();
    let protocol = assets::active_content_registry_protocol();
    let registry = assets::read_registry_for_protocol(registry_bytes, protocol).unwrap();
    let records = registry
        .iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:barrel" | "minecraft:trapdoor"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let barrel = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:barrel")
        .unwrap();
    let visual = BlockVisualId(barrel.sequential_id);
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    write(root, "blocks.json", br#"{
        "stone":{"textures":"stone"},"trapdoor":{"textures":"trapdoor"},
        "barrel":{"textures":{"north":"barrel_top","south":"barrel_bottom","west":"barrel_side","east":"barrel_side","up":"barrel_side","down":"barrel_side"}}
    }"#);
    write(root, "textures/terrain_texture.json", br#"{"texture_data":{
        "stone":{"textures":"textures/blocks/stone"},
        "trapdoor":{"textures":"textures/blocks/trapdoor"},
        "barrel_top":{"textures":["textures/blocks/barrel_top","textures/blocks/barrel_top_open"]},
        "barrel_bottom":{"textures":["textures/blocks/barrel_bottom","textures/blocks/barrel_bottom"]},
        "barrel_side":{"textures":["textures/blocks/barrel_side","textures/blocks/barrel_side"]}
    }}"#);
    write(root, "textures/flipbook_textures.json", b"[]");
    for (index, name) in [
        "stone",
        "trapdoor",
        "barrel_top",
        "barrel_top_open",
        "barrel_bottom",
        "barrel_side",
    ]
    .into_iter()
    .enumerate()
    {
        image::RgbaImage::from_pixel(
            assets::TILE_SIZE,
            assets::TILE_SIZE,
            image::Rgba([20 + index as u8 * 30, 80, 160, 255]),
        )
        .save(root.join(format!("textures/blocks/{name}.png")))
        .unwrap();
    }
    write(
        root,
        "biomes/plains.client_biome.json",
        br#"{"minecraft:client_biome":{"description":{"identifier":"plains"},"components":{}}}"#,
    );
    write(root, "biomes/plains.biome.json", br#"{"minecraft:biome":{"description":{"identifier":"plains"},"components":{"minecraft:climate":{"temperature":0.5,"downfall":0.5}}}}"#);
    fs::create_dir_all(root.join("textures/colormap")).unwrap();
    for map in assets::TintMapId::ALL {
        image::RgbImage::new(assets::TINT_MAP_SIZE, assets::TINT_MAP_SIZE)
            .save(root.join(format!("textures/colormap/{}.png", map.source_name())))
            .unwrap();
    }
    let mut compiled = compile_pack_with_biomes(
        root,
        root,
        &records,
        &[],
        &vec![assets::LightProperties::default(); span],
        protocol,
    )
    .unwrap();
    compiled.biomes = CompiledBiomeAssets::diagnostic();
    (compiled, visual)
}

#[test]
fn barrel_models_still_read_carried_texture_overrides() {
    let pack = tempfile::tempdir().unwrap();
    let root = pack.path();
    let (mut world, visual) = barrel_world(root);
    for (path, bytes) in [
        ("entity/item.json", &br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#[..]),
        ("models/entity/item.json", &br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#[..]),
        ("animations/empty.json", &br#"{"format_version":"1.8.0","animations":{}}"#[..]),
        ("animation_controllers/empty.json", &br#"{"format_version":"1.10.0","animation_controllers":{}}"#[..]),
        ("render_controllers/item.json", &br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#[..]),
        ("textures/entity/item.png", &b"unused-entity-raster"[..]),
    ] {
        write(root, path, bytes);
    }
    let manifest = include_bytes!("../../../../../assets/vanilla-source.json");
    let entity = compile_entity_assets(root, manifest).unwrap();
    let mut visuals = world.visuals.into_vec();
    visuals.resize(
        entity.block_visual_count as usize,
        assets::BlockVisual::diagnostic(
            assets::BlockFlags::empty(),
            assets::ContributorRole::Primary,
        ),
    );
    world.visuals = visuals.into();
    let mut lights = world.light_properties.into_vec();
    lights.resize(
        entity.block_visual_count as usize,
        assets::LightProperties::default(),
    );
    world.light_properties = lights.into();
    world.provenance.source_manifest_sha256 = entity.source_manifest_sha256;
    world.provenance.block_registry_sha256 =
        Sha256::digest(assets::pinned_block_registry_bytes()).into();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&world).unwrap()).unwrap();
    let path = root.join("blocks.json");
    let mut entries: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    entries["barrel"]["carried_textures"] = "trapdoor".into();
    fs::write(path, serde_json::to_vec(&entries).unwrap()).unwrap();
    let compiled = compile_icon_assets_with_blocks(root, manifest, &runtime).unwrap();
    let catalog = RuntimeIconCatalog::decode(&compiled.bytes).unwrap();
    let sheet = catalog
        .block_sheets()
        .iter()
        .find(|sheet| sheet.visual == visual)
        .expect("full-cube barrel admits carried artwork");
    let sprite = &catalog.sprites()[sheet.sprite as usize];
    for face in 0..assets::BlockFace::ALL.len() {
        let side = usize::from(assets::BLOCK_ITEM_FACE_SIDE);
        let columns = usize::from(assets::BLOCK_ITEM_SHEET_GRID[0]);
        let offset =
            ((face / columns * side) * usize::from(sprite.width) + face % columns * side) * 4;
        assert_eq!(
            sprite.rgba8[offset], 50,
            "carried sprite overrides world artwork"
        );
    }
}
