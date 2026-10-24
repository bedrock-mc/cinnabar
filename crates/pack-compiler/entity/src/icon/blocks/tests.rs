use super::*;

#[test]
fn glass_panes_are_models_while_metal_bars_remain_flat() {
    let blocks = IconBlocks::read(tempfile::tempdir().unwrap().path()).unwrap();
    let world = RuntimeAssets::diagnostic();
    let panes: Vec<_> = blocks
        .records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Pane)
        .collect();
    assert!(!panes.is_empty());
    for record in panes {
        assert_eq!(
            blocks.is_flat(&world, BlockVisualId(record.sequential_id)),
            record.name.ends_with("_bars"),
            "{}",
            record.name
        );
    }
}

#[test]
fn crystal_and_spike_items_use_the_flat_inventory_variant() {
    let records = read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    let selected: Vec<_> = records
        .iter()
        .filter(|record| {
            assets::facts::block_render_fact(&record.name).kind
                == assets::facts::BlockRenderKind::Crystal
                || record.traits.contains(BlockTraits::DRIPSTONE_OFFSET)
        })
        .collect();
    assert!(!selected.is_empty());
    let mut catalog = serde_json::Map::new();
    for record in &selected {
        catalog.insert(
            record.name.strip_prefix("minecraft:").unwrap().into(),
            serde_json::json!({"textures": "tile"}),
        );
    }
    let root = tempfile::tempdir().unwrap();
    write_pack(
        root.path(),
        &serde_json::Value::Object(catalog).to_string(),
        r#"{"texture_data":{"tile":{"textures":["textures/blocks/inventory","textures/blocks/placed"]}}}"#,
        "[]",
    );
    let blocks = IconBlocks::read(root.path()).unwrap();
    let world = RuntimeAssets::diagnostic();
    for record in selected {
        let visual = BlockVisualId(record.sequential_id);
        assert!(blocks.is_flat(&world, visual), "{}", record.name);
        assert_eq!(
            blocks.texture_path(visual).as_deref(),
            Some("textures/blocks/inventory"),
            "{} {}",
            record.name,
            record.canonical_state
        );
    }
}

#[test]
fn candles_keep_their_carried_icons_without_world_model_geometry() {
    let blocks = IconBlocks {
        records: assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap(),
        pack: None,
    };
    let world = RuntimeAssets::diagnostic();
    for name in [
        "minecraft:candle",
        "minecraft:magenta_candle",
        "minecraft:bell",
    ] {
        let record = blocks
            .records
            .iter()
            .find(|record| record.name.as_ref() == name)
            .unwrap();
        assert!(blocks.is_flat(&world, BlockVisualId(record.sequential_id)));
    }
}

/// Barrier declares world textures, while light blocks declare only carried ones; both items
/// draw that texture flat.
#[test]
fn barrier_and_light_block_items_draw_their_carried_texture_flat() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("textures")).unwrap();
    for (path, json) in [
        (
            "blocks.json",
            r#"{"barrier":{"textures":"barrier"},
                "light_block_0":{"carried_textures":"light_block_0"},
                "light_block_15":{"carried_textures":"light_block_15"},
                "stone":{"textures":"stone"}}"#,
        ),
        (
            "textures/terrain_texture.json",
            r#"{"texture_data":{
                "barrier":{"textures":"textures/blocks/barrier"},
                "light_block_0":{"textures":"textures/items/light_block_0"},
                "light_block_15":{"textures":"textures/items/light_block_15"},
                "stone":{"textures":"textures/blocks/stone"}}}"#,
        ),
        ("textures/flipbook_textures.json", "[]"),
    ] {
        std::fs::write(root.path().join(path), json).unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    let world = RuntimeAssets::diagnostic();
    let flat_textures = |name: &str| {
        let states: Vec<_> = blocks
            .records
            .iter()
            .filter(|record| record.name.as_ref() == name)
            .map(|record| BlockVisualId(record.sequential_id))
            .collect();
        assert!(!states.is_empty(), "{name} is not registered");
        states
            .into_iter()
            .map(|state| {
                blocks
                    .is_flat(&world, state)
                    .then(|| blocks.texture_path(state))
                    .flatten()
            })
            .collect::<Vec<_>>()
    };
    for (name, texture) in [
        ("minecraft:barrier", "textures/blocks/barrier"),
        ("minecraft:light_block_0", "textures/items/light_block_0"),
        ("minecraft:light_block_15", "textures/items/light_block_15"),
    ] {
        assert!(
            flat_textures(name)
                .iter()
                .all(|path| path.as_deref() == Some(texture)),
            "{name} must draw {texture} flat"
        );
    }
    assert!(flat_textures("minecraft:stone").iter().all(Option::is_none));
}

/// Egg and small creature blocks keep their carried art independent of placed geometry.
#[test]
fn eggs_and_small_creatures_use_the_carried_sprite() {
    let root = tempfile::tempdir().unwrap();
    write_pack(
        root.path(),
        r#"{
            "turtle_egg":{"textures":"world","carried_textures":"carried"},
            "sniffer_egg":{"textures":"world","carried_textures":"carried"},
            "frog_spawn":{"textures":"carried"}
        }"#,
        r#"{"texture_data":{
            "world":{"textures":"textures/blocks/world"},
            "carried":{"textures":"textures/items/carried"}
        }}"#,
        "[]",
    );
    let blocks = IconBlocks::read(root.path()).unwrap();
    let world = RuntimeAssets::diagnostic();
    for name in ["turtle_egg", "sniffer_egg", "frog_spawn"] {
        let states: Vec<_> = blocks
            .records
            .iter()
            .filter(|record| record.name.as_ref() == format!("minecraft:{name}"))
            .collect();
        assert!(!states.is_empty(), "registered block {name}");
        for record in states {
            let visual = BlockVisualId(record.sequential_id);
            assert!(
                blocks.is_flat(&world, visual),
                "{name} uses its carried sprite"
            );
            assert_eq!(
                blocks.texture_path(visual).as_deref(),
                Some("textures/items/carried")
            );
        }
    }
}

#[test]
fn leaf_litter_uses_a_flat_carried_icon_for_every_state() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join("textures")).unwrap();
    std::fs::write(
        root.join("blocks.json"),
        r#"{"leaf_litter":{"textures":"ground","carried_textures":"carried"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"ground":{"textures":"textures/blocks/litter"},"carried":{"textures":"textures/items/litter"}}}"#).unwrap();
    std::fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    let blocks = IconBlocks::read(root).unwrap();
    let world = RuntimeAssets::diagnostic();
    let records: Vec<_> = blocks
        .records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:leaf_litter")
        .collect();
    assert!(!records.is_empty());
    for record in records {
        assert!(
            blocks.is_flat(&world, BlockVisualId(record.sequential_id)),
            "{}",
            record.canonical_state
        );
        assert_eq!(
            blocks
                .texture_path(BlockVisualId(record.sequential_id))
                .as_deref(),
            Some("textures/items/litter")
        );
    }
}

/// Inventory furnace faces use authored slots regardless of the placed state's direction.
#[test]
fn furnace_inventory_faces_show_the_unrotated_front() {
    let root = tempfile::tempdir().unwrap();
    write_pack(
        root.path(),
        r#"{
            "furnace":{"textures":{"up":"top","down":"top","north":"side","south":"front","west":"side","east":"side"}},
            "blast_furnace":{"textures":{"up":"top","down":"top","north":"side","south":"front","west":"side","east":"side"}},
            "smoker":{"textures":{"up":"top","down":"top","north":"side","south":"front","west":"side","east":"side"}}
        }"#,
        r#"{"texture_data":{
            "top":{"textures":"textures/blocks/top"},
            "side":{"textures":"textures/blocks/side"},
            "front":{"textures":"textures/blocks/front"}
        }}"#,
        "[]",
    );
    for (name, pixel) in [
        ("top", [19, 31, 47, 255]),
        ("side", [61, 79, 97, 255]),
        ("front", [101, 127, 151, 255]),
    ] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(pixel))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    for name in [
        "minecraft:furnace",
        "minecraft:blast_furnace",
        "minecraft:smoker",
    ] {
        let states: Vec<_> = blocks
            .records
            .iter()
            .filter(|r| r.name.as_ref() == name)
            .collect();
        assert!(!states.is_empty(), "registered block {name}");
        for record in states {
            let tiles = blocks
                .carried_cube_tiles(root.path(), BlockVisualId(record.sequential_id))
                .unwrap()
                .unwrap();
            assert_eq!(
                &tiles[BlockFace::South as usize].rgba8[..4],
                &[101, 127, 151, 255],
                "front of {name} in {}",
                record.canonical_state
            );
            assert_eq!(
                &tiles[BlockFace::West as usize].rgba8[..4],
                &[61, 79, 97, 255]
            );
            assert_eq!(
                &tiles[BlockFace::Up as usize].rgba8[..4],
                &[19, 31, 47, 255]
            );
        }
    }
}

#[test]
fn barrel_inventory_faces_use_the_closed_south_facing_pose() {
    let root = tempfile::tempdir().unwrap();
    write_pack(
        root.path(),
        r#"{"barrel":{"textures":{"up":"side","down":"side","north":"top","south":"bottom","west":"side","east":"side"}}}"#,
        r#"{"texture_data":{"top":{"textures":"textures/blocks/top"},"bottom":{"textures":"textures/blocks/bottom"},"side":{"textures":"textures/blocks/side"}}}"#,
        "[]",
    );
    for (name, pixel) in [
        ("top", [180, 30, 40, 255]),
        ("bottom", [30, 180, 40, 255]),
        ("side", [30, 40, 180, 255]),
    ] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(pixel))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    let mut seen = 0;
    for record in blocks
        .records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:barrel")
    {
        let tiles = blocks
            .carried_cube_tiles(root.path(), BlockVisualId(record.sequential_id))
            .unwrap()
            .unwrap();
        assert_eq!(
            &tiles[BlockFace::South as usize].rgba8[..4],
            &[180, 30, 40, 255],
            "{}",
            record.canonical_state
        );
        assert_eq!(
            &tiles[BlockFace::Up as usize].rgba8[..4],
            &[30, 40, 180, 255]
        );
        seen += 1;
    }
    assert!(seen > 1);
}

#[test]
fn partial_furnace_carried_faces_keep_the_world_fallback() {
    let root = tempfile::tempdir().unwrap();
    write_pack(
        root.path(),
        r#"{"furnace":{"textures":"side","carried_textures":{"up":"top"}}}"#,
        r#"{"texture_data":{"side":{"textures":"textures/blocks/side"},"top":{"textures":"textures/blocks/top"}}}"#,
        "[]",
    );
    for (name, color) in [("side", [61, 79, 97, 255]), ("top", [19, 31, 47, 255])] {
        image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    for record in blocks
        .records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:furnace")
    {
        let tiles = blocks
            .carried_cube_tiles(root.path(), BlockVisualId(record.sequential_id))
            .unwrap()
            .expect("missing carried faces fall back independently");
        for face in BlockFace::ALL {
            let expected = if face == BlockFace::Up {
                [19, 31, 47, 255]
            } else {
                [61, 79, 97, 255]
            };
            assert_eq!(&tiles[face as usize].rgba8[..4], &expected);
        }
    }
}

/// Writes the block and texture catalogs needed to load icon test fixtures.
pub(super) fn write_pack(root: &Path, blocks: &str, terrain: &str, flipbooks: &str) {
    std::fs::create_dir_all(root.join("textures/blocks")).unwrap();
    std::fs::write(root.join("blocks.json"), blocks).unwrap();
    std::fs::write(root.join("textures/terrain_texture.json"), terrain).unwrap();
    std::fs::write(root.join("textures/flipbook_textures.json"), flipbooks).unwrap();
}

/// A flat icon shows one frame of a flipbook strip, while other textures keep their size.
#[test]
fn flat_flipbook_icons_show_their_first_frame() {
    let root = tempfile::tempdir().unwrap();
    write_pack(
        root.path(),
        r#"{"sculk_vein":{"textures":"sculk_vein"}}"#,
        r#"{"texture_data":{"sculk_vein":{"textures":"textures/blocks/sculk_vein"}}}"#,
        r#"[{"flipbook_texture":"textures/blocks/sculk_vein","atlas_tile":"sculk_vein","ticks_per_frame":2}]"#,
    );
    for name in ["sculk_vein", "tall"] {
        image::RgbaImage::new(16, 64)
            .save(root.path().join(format!("textures/blocks/{name}.png")))
            .unwrap();
    }
    let blocks = IconBlocks::read(root.path()).unwrap();
    let size = |path: &str| {
        let sprite = blocks.flat_sprite(root.path(), path).unwrap().unwrap();
        [sprite.width, sprite.height]
    };
    assert_eq!(size("textures/blocks/sculk_vein"), [16, 16]);
    assert_eq!(size("textures/blocks/tall"), [16, 64]);
}

/// A model texture past the carrier's side is skipped rather than failing its decode.
#[test]
fn oversized_model_textures_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    image::RgbaImage::new(512, 512)
        .save(root.path().join("head.png"))
        .unwrap();
    assert!(
        IconBlocks::model_texture(root.path(), "head")
            .unwrap()
            .is_none()
    );
}
