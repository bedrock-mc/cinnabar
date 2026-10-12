use super::*;

#[test]
fn potted_models_sample_the_exposed_server_texture_grid() {
    let side = 64;
    let pixels = image::RgbaImage::from_fn(side, side, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 4) as u8, 91, 255])
    })
    .into_raw();
    let quad = assets::ModelQuad {
        positions: [[36, 256, 36], [220, 256, 220], [220, 64, 220], [36, 64, 36]],
        uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
        material: 0,
        flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
    };
    let cropped = assets::ModelQuad {
        uvs: [[512, 1024], [3584, 1024], [3584, 3072], [512, 3072]],
        ..quad
    };
    for grid in [0, 1, 2] {
        let overlay = assets::BlockOverlay {
            visuals: vec![assets::BlockVisual {
                faces: [0; 6],
                flags: assets::BlockFlags::empty(),
                kind: VisualKind::Model,
                support: assets::VisualSupport::VanillaFallback,
                contributor_role: assets::ContributorRole::Primary,
                model_template: 1,
                animation: assets::NO_ANIMATION,
                variant: 0,
                destroy: assets::compiled::block_destroy::BlockDestroyFacts::Unresolved,
            }],
            light_properties: vec![assets::LightProperties::default()],
            materials: vec![assets::Material {
                texture: assets::TextureRef::new(1, 0).unwrap(),
                ..assets::Material::unvaried()
            }],
            model_templates: vec![
                assets::ModelTemplate {
                    quad_start: 0,
                    quad_count: 2,
                    flags: assets::MODEL_TEMPLATE_FLAG_POTTED_PLANT,
                },
                assets::ModelTemplate {
                    quad_start: 2,
                    quad_count: 1,
                    flags: 0,
                },
            ],
            model_quads: vec![quad, cropped, quad],
            texture: Some(assets::TextureArray {
                layers: 1,
                mips: assets::build_texture_mip_chain(pixels.clone().into(), side).unwrap(),
            }),
            texture_source_sizes: vec![[side as u16 >> grid; 2]],
            texture_source_grids: vec![grid],
            ..assets::BlockOverlay::default()
        };
        let runtime = RuntimeAssets::diagnostic()
            .with_block_overlay(1, &overlay)
            .unwrap();
        let mut cache = ModelCache::default();
        cache.begin_frame();
        cache.finish_demand();
        let handle = DroppedItemPublisher::potted_model(&mut cache, &runtime, 1).unwrap();
        let DroppedItemModel::Block(model) = &cache.models[handle as usize] else {
            panic!("pot contents must retain their model");
        };
        assert_eq!(model.materials.len(), 1);
        assert_eq!(model.quads.len(), 2);
        let sprite = &model.materials[0].0;
        assert_eq!(sprite.rgba8.as_ref(), pixels);
        for (retained, authored) in model.quads.iter().zip([quad, cropped]) {
            assert_eq!(retained.positions, authored.positions);
            // Sample interior points in both axes to detect flips and incorrect grid regions.
            for (u, v) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let sample = |uv: [[u16; 2]; 4], scale: f32| {
                    let x = (f32::from(uv[0][0]) * (1.0 - u) + f32::from(uv[1][0]) * u) * scale;
                    let y = (f32::from(uv[0][1]) * (1.0 - v) + f32::from(uv[3][1]) * v) * scale;
                    (x as usize, y as usize)
                };
                let (x, y) = sample(retained.uvs, side as f32 / 4096.0);
                let (expected_x, expected_y) = sample(authored.uvs, (side >> grid) as f32 / 4096.0);
                let offset = (y * side as usize + x) * 4;
                assert_eq!(
                    &sprite.rgba8[offset..offset + 4],
                    &[(expected_x * 4) as u8, (expected_y * 4) as u8, 91, 255],
                    "grid {grid}, sample ({u}, {v})"
                );
            }
        }
    }
}

#[test]
fn falling_blocks_keep_non_cube_world_geometry_and_cube_controls() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("assets/bedrock-target.json")).unwrap())
            .unwrap();
    let path = root.join(target["artifacts"]["world_assets"].as_str().unwrap());
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "SKIP falling_blocks_keep_non_cube_world_geometry_and_cube_controls: missing {}",
            path.display()
        );
        return;
    };
    let assets = RuntimeAssets::decode(&bytes).unwrap();
    let records = assets::read_registry_for_protocol(
        &std::fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        target["wire_protocol"].as_u64().unwrap() as u32,
    )
    .unwrap();
    let mut cache = ModelCache::default();
    cache.begin_frame();
    cache.finish_demand();
    for name in ["minecraft:dragon_egg", "minecraft:sand", "minecraft:gravel"] {
        let id = records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap()
            .sequential_id;
        assert!(
            DroppedItemPublisher::block_model(&mut cache, &assets, NetworkIdMode::Sequential, id)
                .is_some(),
            "falling {name} must retain its authored world geometry"
        );
        if name == "minecraft:dragon_egg" {
            let DroppedItemModel::Block(model) = cache.models.last().unwrap() else {
                panic!("egg requires its stepped model")
            };
            assert_eq!(model.quads.len(), 48);
            assert!(
                model
                    .quads
                    .iter()
                    .any(|quad| quad.positions.iter().any(|p| p[1] == 240))
            );
        }
    }
}

fn stream(hashes: bool, internal: u32) -> WorldStream {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: hashes,
    });
    stream.set_custom_block_ids(internal..internal + 1);
    if !hashes {
        stream.set_sequential_id_remap(assets::SequentialIdRemap::from_palette(
            vec![0, internal],
            internal + 1,
        ));
    }
    stream
}

#[test]
fn retained_items_and_falling_blocks_resolve_the_current_session_palette() {
    let visual = ItemVisualRoute::RetainedBlock {
        block_runtime_id: 1,
    };
    let falling = BlockEntityKind::Falling {
        block_runtime_id: 1,
    };
    for internal in [7, 6] {
        let stream = stream(false, internal);
        assert_eq!(
            item_block_id(&stream, visual),
            Some((NetworkIdMode::Sequential, internal))
        );
        assert_eq!(
            entity_block_id(&stream, &falling),
            Some((NetworkIdMode::Sequential, internal, 1.0)),
        );
    }
    assert_eq!(
        visual,
        ItemVisualRoute::RetainedBlock {
            block_runtime_id: 1
        }
    );
    assert_eq!(
        falling,
        BlockEntityKind::Falling {
            block_runtime_id: 1
        }
    );
}

#[test]
fn retained_items_and_falling_blocks_preserve_high_bit_hashes() {
    let hash = 0x8000_0007_u32;
    let stream = stream(true, hash);
    let raw = i32::from_ne_bytes(hash.to_ne_bytes());
    assert!(raw < 0);
    assert_eq!(
        item_block_id(
            &stream,
            ItemVisualRoute::RetainedBlock {
                block_runtime_id: raw
            }
        ),
        Some((NetworkIdMode::Hashed, hash)),
    );
    assert_eq!(
        entity_block_id(
            &stream,
            &BlockEntityKind::Falling {
                block_runtime_id: raw
            }
        ),
        Some((NetworkIdMode::Hashed, hash, 1.0)),
    );
}

#[test]
fn compiled_block_item_and_tnt_visuals_are_already_internal() {
    let stream = stream(false, 7);
    let visual = ItemVisualRoute::BlockItem(assets::BlockVisualId(1));
    assert_eq!(
        item_block_id(&stream, visual),
        Some((NetworkIdMode::Sequential, 1))
    );
    assert_eq!(
        entity_block_id(&stream, &BlockEntityKind::PrimedTnt { visual }),
        Some((NetworkIdMode::Sequential, 1, 1.0)),
    );
}

use assets::{CompiledAssets, RegistryRecord};
use std::fs;
/// Compiles real barrel selectors with original opaque sprite artwork for held-consumer checks.
fn barrels() -> (CompiledAssets, Vec<RegistryRecord>) {
    barrels_with_size(assets::TILE_SIZE)
}

/// Compiles original sprites at the requested source resolution.
fn barrels_with_size(side: u32) -> (CompiledAssets, Vec<RegistryRecord>) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("biomes")).unwrap();
    fs::create_dir_all(root.join("behavior/biomes")).unwrap();
    fs::create_dir_all(root.join("textures/colormap")).unwrap();
    fs::write(
        root.join("biomes/plain.client_biome.json"),
        r#"{"minecraft:client_biome":{"description":{"identifier":"plain"},"components":{}}}"#,
    )
    .unwrap();
    fs::write(root.join("behavior/biomes/plain.biome.json"),r#"{"minecraft:biome":{"description":{"identifier":"plain"},"components":{"minecraft:climate":{"temperature":0.8,"downfall":0.4}}}}"#).unwrap();
    fs::write(root.join("blocks.json"), r#"{"barrel":{"textures":"art"}}"#).unwrap();
    fs::write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"art":{"textures":"textures/art"}}}"#,
    )
    .unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    image::RgbaImage::from_fn(side, side, |x, y| {
        image::Rgba([20, (x * 192 / side) as u8, (y * 192 / side) as u8, 255])
    })
    .save(root.join("textures/art.png"))
    .unwrap();
    for map in assets::TintMapId::ALL {
        image::RgbaImage::from_pixel(
            assets::TINT_MAP_SIZE,
            assets::TINT_MAP_SIZE,
            image::Rgba([255; 4]),
        )
        .save(root.join(format!("textures/colormap/{}.png", map.source_name())))
        .unwrap();
    }
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap()
            .into_vec()
            .into_iter()
            .filter(|r| matches!(r.name.as_ref(), "minecraft:air" | "minecraft:barrel"))
            .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let compiled = pack_compiler::compile_pack_with_biomes(
        root,
        &root.join("behavior"),
        &records,
        &[],
        &vec![assets::LightProperties::default(); span],
        protocol,
    )
    .unwrap();
    (compiled, records)
}

#[test]
fn dropped_barrel_models_use_authored_carried_faces_for_both_id_modes() {
    let (compiled, records) = barrels();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let record = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:barrel")
        .unwrap();
    let tiles = std::array::from_fn(|face| assets::IconSprite {
        width: assets::BLOCK_ITEM_FACE_SIDE,
        height: assets::BLOCK_ITEM_FACE_SIDE,
        rgba8: [91 + face as u8, 40, 80, 255]
            .repeat(usize::from(assets::BLOCK_ITEM_FACE_SIDE).pow(2))
            .into(),
    });
    let sprite = assets::compose_block_item_sheet(&tiles).unwrap();
    let catalog = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog_with_block_sheets(
            runtime.provenance().source_manifest_sha256,
            &[sprite],
            &[],
            &[assets::IconBlockSheet {
                visual: assets::BlockVisualId(record.sequential_id),
                sprite: 0,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let icons = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
        Arc::new(catalog),
    )
    .unwrap();
    for (mode, id) in [
        (NetworkIdMode::Sequential, record.sequential_id),
        (NetworkIdMode::Hashed, record.network_hash),
    ] {
        let mut cache = ModelCache::default();
        cache.begin_frame();
        cache.finish_demand();
        let handle =
            DroppedItemPublisher::carried_block_model(&mut cache, &icons, &runtime, mode, id)
                .expect("full-cube barrel models retain authored dropped faces");
        let DroppedItemModel::Cube(cube) = &cache.models[handle as usize] else {
            panic!("carried cube");
        };
        for (face, pixels) in cube.faces.iter().enumerate() {
            assert_eq!(pixels.as_ref(), tiles[face].rgba8.as_ref());
        }
    }
}
