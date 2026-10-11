use super::*;
use assets::{LightProperties, encode_blob};
use serde_json::{Value, json};
use std::fs;

/// Loads the active registry and lighting with only the anchor and required air/stone records.
fn records() -> (Vec<RegistryRecord>, Box<[LightProperties]>, u32) {
    let target: Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
    let all = read_registry_for_protocol(&bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &bytes,
        all.len(),
        protocol,
    )
    .unwrap();
    let selected: Vec<_> = all
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:stone" | "minecraft:respawn_anchor"
            )
        })
        .collect();
    let span = selected
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    (selected, lights[..span].into(), protocol)
}

/// Writes original colored faces and top strips, including distinct charge flipbook selectors.
fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("textures/blocks")).unwrap();
    fs::create_dir_all(root.join("textures/colormap")).unwrap();
    fs::create_dir_all(root.join("biomes")).unwrap();
    fs::write(
        root.join("biomes/plains.client_biome.json"),
        json!({
            "minecraft:client_biome":{"description":{"identifier":"minecraft:plains"}}
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        root.join("biomes/plains.biome.json"),
        json!({
            "minecraft:biome":{"description":{"identifier":"minecraft:plains"},
            "components":{"minecraft:climate":{"temperature":0.5,"downfall":0.5}}}
        })
        .to_string(),
    )
    .unwrap();
    for map in assets::TintMapId::ALL {
        let name = map.source_name();
        image::RgbImage::from_pixel(
            assets::TINT_MAP_SIZE,
            assets::TINT_MAP_SIZE,
            image::Rgb([255; 3]),
        )
        .save(root.join(format!("textures/colormap/{name}.png")))
        .unwrap();
    }
    fs::write(
        root.join("blocks.json"),
        json!({
            "stone": {"textures":"bottom"},
            "respawn_anchor": {"textures":{"down":"bottom","up":"top","side":"side"}}
        })
        .to_string(),
    )
    .unwrap();
    fs::write(root.join("textures/terrain_texture.json"), json!({"texture_data":{
        "bottom":{"textures":vec!["textures/blocks/bottom"; 5]},
        "top":{"textures":["textures/blocks/off","textures/blocks/on","textures/blocks/on","textures/blocks/on","textures/blocks/on"]},
        "side":{"textures":(0..5).map(|i| format!("textures/blocks/side{i}")).collect::<Vec<_>>()}
    }}).to_string()).unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), Value::Array((0..5).map(|i| json!({
        "flipbook_texture": if i == 0 {"textures/blocks/off"} else {"textures/blocks/on"},
        "atlas_tile":"top", "atlas_index":i, "atlas_tile_variant":i,
        "frames":[i % 2, 1 - i % 2], "blend_frames":false
    })).collect()).to_string()).unwrap();
    for (name, color, frames) in [
        ("bottom".to_owned(), [31, 47, 59, 255], 1),
        ("off".to_owned(), [79, 17, 139, 255], 32),
        ("on".to_owned(), [229, 191, 53, 255], 32),
    ]
    .into_iter()
    .chain((0..5).map(|i| (format!("side{i}"), [30 + 40 * i, 83, 127, 255], 1)))
    {
        image::RgbaImage::from_fn(16, 16 * frames, |_, y| {
            image::Rgba(if y < 16 { color } else { [11, 37, 59, 255] })
        })
        .save(root.join(format!("textures/blocks/{name}.png")))
        .unwrap();
    }
    directory
}

/// Compiles the original fixture through production world-carrier encoding and decoding.
fn world(
    root: &Path,
    records: &[RegistryRecord],
    lights: &[LightProperties],
    protocol: u32,
) -> RuntimeAssets {
    let compiled =
        crate::compile_pack_with_biomes(root, root, records, &[], lights, protocol).unwrap();
    RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap()
}

#[test]
fn respawn_anchor_charge_faces_and_flipbooks_survive_the_world_carrier() {
    let (records, lights, protocol) = records();
    let directory = fixture();
    let world = world(directory.path(), &records, &lights, protocol);
    let anchors = records
        .iter()
        .filter(|r| r.name.as_ref() == "minecraft:respawn_anchor");
    assert_eq!(anchors.clone().count(), 5);
    for record in anchors {
        let charge =
            serde_json::from_str::<Value>(&record.canonical_state).unwrap()["respawn_anchor_charge"]
                ["value"]
                .as_u64()
                .unwrap() as u32;
        let block = world.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(block.kind(), VisualKind::Cube, "charge {charge}");
        assert_eq!(block.support(), VisualSupport::Exact, "charge {charge}");
        assert!(block.flags().contains(BlockFlags::CUBE_GEOMETRY));
        assert!(!block.flags().contains(BlockFlags::OCCLUDES_FULL_FACE));
        assert_eq!(
            block.light_properties().emission(),
            [0, 3, 7, 11, 15][charge as usize]
        );
        for face in BlockFace::ALL {
            let material = &world.materials()[block.face(face).material_id() as usize];
            assert_eq!(material.flags & assets::MATERIAL_FLAG_ALPHA_BLEND, 0);
            if face == BlockFace::Up {
                let animation = &world.animations()[material.animation as usize];
                assert_eq!(animation.atlas_index, charge);
                assert_eq!(animation.atlas_tile_variant, charge);
            } else {
                let mip = &world.texture_pages()[material.texture.page() as usize]
                    .texture
                    .mips[0];
                let offset = material.texture.layer() as usize * (mip.size * mip.size * 4) as usize;
                let expected = if face == BlockFace::Down {
                    [31, 47, 59, 255]
                } else {
                    [30 + 40 * charge as u8, 83, 127, 255]
                };
                assert_eq!(&mip.rgba8[offset..offset + 4], expected);
            }
        }
    }
}

#[test]
fn respawn_anchor_carried_cube_preserves_six_pack_faces_including_animated_top() {
    let (records, lights, protocol) = records();
    let directory = fixture();
    let world = world(directory.path(), &records, &lights, protocol);
    let blocks = IconBlocks::read(directory.path()).unwrap();
    let record = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:respawn_anchor")
        .unwrap();
    let id = BlockVisualId(record.sequential_id);
    assert!(blocks.is_carried_cube(&world, id));
    let tiles = blocks
        .carried_cube_tiles(directory.path(), id)
        .unwrap()
        .expect("the held anchor has a six-face cube sheet");
    for face in BlockFace::ALL {
        let expected = match face {
            BlockFace::Down => [31, 47, 59, 255],
            BlockFace::Up => [79, 17, 139, 255],
            _ => [30, 83, 127, 255],
        };
        assert_eq!(&tiles[face as usize].rgba8[..4], expected);
    }
    assert!(assets::compose_block_item_sheet(&tiles).is_some());
}

#[test]
fn respawn_anchor_short_arrays_fall_back_to_first_face_and_scalar_overrides_survive() {
    let (records, lights, protocol) = records();
    let directory = fixture();
    let catalog = directory.path().join("textures/terrain_texture.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&catalog).unwrap()).unwrap();
    document["texture_data"]["bottom"]["textures"] = json!("textures/blocks/bottom");
    document["texture_data"]["side"]["textures"] =
        json!(["textures/blocks/side0", "textures/blocks/side1"]);
    fs::write(catalog, document.to_string()).unwrap();
    let world = world(directory.path(), &records, &lights, protocol);
    let pack = read_pack(directory.path()).unwrap();
    for record in records
        .iter()
        .filter(|r| crate::respawn_anchor::is_record(r))
    {
        let charge = crate::respawn_anchor::charge(record).unwrap();
        let selected = if charge < 2 { charge } else { 0 };
        assert_eq!(
            pack.terrain.get_for_model_record("side", record),
            Some((
                format!("textures/blocks/side{selected}").as_str(),
                selected as u32
            ))
        );
        assert_eq!(
            pack.terrain.get_for_record_variant("side", record),
            pack.terrain.get_for_model_record("side", record)
        );
        assert_eq!(
            pack.terrain.get_for_record_variant("bottom", record),
            Some(("textures/blocks/bottom", 0))
        );
        assert_eq!(
            world
                .resolve(NetworkIdMode::Sequential, record.sequential_id)
                .support(),
            VisualSupport::Exact
        );
    }
    let stone = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:stone")
        .unwrap();
    assert_eq!(
        world
            .resolve(NetworkIdMode::Sequential, stone.sequential_id)
            .kind(),
        VisualKind::Cube
    );
    assert_eq!(
        pack.terrain.get_for_record_variant("side", stone),
        Some(("textures/blocks/side0", 0))
    );
}

#[test]
fn respawn_anchor_malformed_charge_states_do_not_gain_exact_cube_admission() {
    let (mut records, lights, protocol) = records();
    let directory = fixture();
    let index = records
        .iter()
        .position(|r| crate::respawn_anchor::is_record(r))
        .unwrap();
    for state in [
        json!({}),
        json!({"respawn_anchor_charge":{"type":"int","value":5}}),
        json!({"respawn_anchor_charge":{"type":"int","value":-1}}),
        json!({"respawn_anchor_charge":{"type":"byte","value":1}}),
        json!({"respawn_anchor_charge":{"type":"int","value":1},"extra":true}),
    ] {
        records[index].canonical_state = state.to_string().into();
        let world = world(directory.path(), &records, &lights, protocol);
        let visual = world.resolve(NetworkIdMode::Sequential, records[index].sequential_id);
        assert_eq!(visual.kind(), VisualKind::Diagnostic);
        assert_eq!(visual.support(), VisualSupport::Diagnostic);
    }
}

#[test]
fn respawn_anchor_transparent_pack_faces_do_not_hide_adjacent_terrain() {
    let (records, lights, protocol) = records();
    let directory = fixture();
    let side = directory.path().join("textures/blocks/side0.png");
    let mut image = image::open(&side).unwrap().into_rgba8();
    image.put_pixel(8, 8, image::Rgba([0; 4]));
    image.save(side).unwrap();
    let world = world(directory.path(), &records, &lights, protocol);
    let record = records
        .iter()
        .find(|r| crate::respawn_anchor::is_record(r))
        .unwrap();
    let anchor = world.resolve(NetworkIdMode::Sequential, record.sequential_id);
    assert_eq!(anchor.kind(), VisualKind::Cube);
    assert!(
        !anchor.flags().contains(BlockFlags::OCCLUDES_FULL_FACE),
        "cutout texels must reveal the neighboring terrain face"
    );
    let material = world.materials()[anchor.face(BlockFace::North).material_id() as usize];
    assert_ne!(material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT, 0);
}

#[test]
fn respawn_anchor_partial_flipbooks_keep_unanimated_charges_drawable() {
    let (records, lights, protocol) = records();
    for horizontal in [false, true] {
        for charges in [
            &[0, 2, 3, 4][..],
            &[0, 4][..],
            &[0][..],
            &[1, 4][..],
            &[][..],
        ] {
            let directory = fixture();
            if horizontal {
                for name in ["off", "on"] {
                    let path = directory.path().join(format!("textures/blocks/{name}.png"));
                    let vertical = image::open(&path).unwrap().to_rgba8();
                    image::RgbaImage::from_fn(vertical.height(), vertical.width(), |x, y| {
                        *vertical.get_pixel(y, x)
                    })
                    .save(path)
                    .unwrap();
                }
            }
            let path = directory.path().join("textures/flipbook_textures.json");
            let sources: Vec<Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let selected: Vec<_> = charges
                .iter()
                .map(|&charge| sources[charge].clone())
                .collect();
            fs::write(&path, serde_json::to_vec(&selected).unwrap()).unwrap();
            let world = world(directory.path(), &records, &lights, protocol);
            let blocks = IconBlocks::read(directory.path()).unwrap();
            for record in records
                .iter()
                .filter(|r| crate::respawn_anchor::is_record(r))
            {
                let charge = crate::respawn_anchor::charge(record).unwrap();
                let anchor = world.resolve(NetworkIdMode::Sequential, record.sequential_id);
                assert_eq!(anchor.kind(), VisualKind::Cube);
                let faces = blocks
                    .carried_cube_tiles(directory.path(), BlockVisualId(record.sequential_id))
                    .unwrap()
                    .expect("each anchor strip layout retains its carried cube sheet");
                let expected_top = if charge == 0 {
                    [79, 17, 139, 255]
                } else {
                    [229, 191, 53, 255]
                };
                assert_eq!(&faces[BlockFace::Up as usize].rgba8[..4], expected_top);
                assert!(assets::compose_block_item_sheet(&faces).is_some());
                let top = world.materials()[anchor.face(BlockFace::Up).material_id() as usize];
                if charges.contains(&charge) {
                    assert_eq!(
                        world.animations()[top.animation as usize].atlas_index,
                        charge as u32
                    );
                } else {
                    assert_eq!(top.animation, assets::NO_ANIMATION);
                    let mip = &world.texture_pages()[top.texture.page() as usize]
                        .texture
                        .mips[0];
                    let offset = top.texture.layer() as usize * (mip.size * mip.size * 4) as usize;
                    let expected = if charge == 0 {
                        [79, 17, 139, 255]
                    } else {
                        [229, 191, 53, 255]
                    };
                    assert_eq!(&mip.rgba8[offset..offset + 4], expected);
                }
            }
        }
    }
}

#[test]
fn respawn_anchor_carried_strips_preserve_first_faces_at_pack_resolutions() {
    let (records, lights, protocol) = records();
    for frames in [2, 3, 4, 32] {
        for side in [16, 32] {
            for horizontal in [false, true] {
                let directory = fixture();
                for (name, color) in [("off", [79, 17, 139, 255]), ("on", [229, 191, 53, 255])] {
                    let (width, height) = if horizontal {
                        (side * frames, side)
                    } else {
                        (side, side * frames)
                    };
                    image::RgbaImage::from_fn(width, height, |x, y| {
                        let frame = if horizontal { x / side } else { y / side };
                        image::Rgba(if frame == 0 { color } else { [11, 37, 59, 255] })
                    })
                    .save(directory.path().join(format!("textures/blocks/{name}.png")))
                    .unwrap();
                }
                let flipbooks = directory.path().join("textures/flipbook_textures.json");
                let mut sources: Vec<Value> =
                    serde_json::from_slice(&fs::read(&flipbooks).unwrap()).unwrap();
                for source in &mut sources {
                    source["frames"] = json!([0]);
                }
                fs::write(flipbooks, serde_json::to_vec(&sources).unwrap()).unwrap();
                let world = world(directory.path(), &records, &lights, protocol);
                let blocks = IconBlocks::read(directory.path()).unwrap();
                for record in records
                    .iter()
                    .filter(|r| crate::respawn_anchor::is_record(r))
                {
                    assert!(blocks.is_carried_cube(&world, BlockVisualId(record.sequential_id)));
                    let tiles = blocks
                        .carried_cube_tiles(directory.path(), BlockVisualId(record.sequential_id))
                        .unwrap()
                        .expect("supported strip layouts retain their cube sheet");
                    let expected = if crate::respawn_anchor::charge(record) == Some(0) {
                        [79, 17, 139, 255]
                    } else {
                        [229, 191, 53, 255]
                    };
                    assert_eq!(&tiles[BlockFace::Up as usize].rgba8[..4], expected);
                    assert!(assets::compose_block_item_sheet(&tiles).is_some());
                }
            }
        }
    }
}

#[test]
fn respawn_anchor_carried_overlay_masks_keep_rgb_when_resized() {
    let directory = tempfile::tempdir().unwrap();
    image::RgbaImage::from_fn(32, 32, |x, _| {
        image::Rgba(if x < 16 {
            [101, 131, 163, 0]
        } else {
            [255, 255, 255, 255]
        })
    })
    .save(directory.path().join("masked.png"))
    .unwrap();
    let tile = crate::icon::carried::tile(directory.path(), "masked", Some("#804020"), true)
        .unwrap()
        .expect("masked carried anchor face remains drawable");
    assert_eq!([tile.width, tile.height], [assets::BLOCK_ITEM_FACE_SIDE; 2]);
    assert_eq!(&tile.rgba8[..4], &[101, 131, 163, 255]);
    let last_pixel = (usize::from(tile.width) - 1) * 4;
    assert_eq!(&tile.rgba8[last_pixel..last_pixel + 4], &[128, 64, 32, 255]);
    assert!(
        tile.rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
}
