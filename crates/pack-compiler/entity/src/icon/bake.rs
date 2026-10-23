//! Bounded block-thumbnail and carried-sheet baking, sharing colored face
//! pixels between inventory thumbnails and runtime held-block geometry.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use assets::{
    AssetError, BlockVisualId, CompiledEntityAssets, EntityGeometry, IconBlockModel,
    IconBlockSheet, IconSprite, NetworkIdMode, RuntimeAssets, compose_block_item_sheet,
};
use sha2::{Digest, Sha256};

use super::{blocks::IconBlocks, cube, model};

pub(super) struct BakedBlocks {
    pub flat_sprites: BTreeMap<u32, u32>,
    pub model_sprites: BTreeMap<(u32, u32), u32>,
    pub block_sprites: BTreeMap<u32, u32>,
    pub block_sheets: Vec<IconBlockSheet>,
    /// Model thumbnails by sprite, with the world state whose quads they project.
    pub block_models: Vec<IconBlockModel>,
}

pub(super) fn run(
    root: &Path,
    world: Option<&RuntimeAssets>,
    blocks: Option<&IconBlocks>,
    flat_plan: &BTreeMap<u32, Option<Box<str>>>,
    block_plan: &BTreeMap<u32, Result<cube::Cube<'_>, cube::Reject>>,
    compiled: &CompiledEntityAssets,
    sprites: &mut Vec<IconSprite>,
) -> Result<BakedBlocks, AssetError> {
    let mut block_models = BTreeMap::new();
    let mut flat_by_path: BTreeMap<Box<str>, Option<u32>> = BTreeMap::new();
    let mut flat_sprites = BTreeMap::new();
    for (&visual, path) in flat_plan {
        let Some(path) = path else {
            continue;
        };
        let sprite = match flat_by_path.get(path) {
            Some(existing) => *existing,
            None => {
                let sprite = blocks
                    .map(|blocks| blocks.flat_sprite(root, path))
                    .transpose()?
                    .flatten()
                    .map(|sprite| insert_sprite(sprites, sprite, &block_models));
                flat_by_path.insert(path.clone(), sprite);
                sprite
            }
        };
        if let Some(sprite) = sprite {
            flat_sprites.insert(visual, sprite);
        }
    }
    let mut carried_tiles = BTreeMap::new();
    if let (Some(world), Some(blocks)) = (world, blocks) {
        for &visual in block_plan.keys() {
            let id = BlockVisualId(visual);
            if !blocks.name(id).is_some_and(super::shapes::has_model)
                && blocks.is_carried_cube(world, id)
                && let Some(tiles) = blocks.carried_cube_tiles(root, id)?
            {
                carried_tiles.insert(visual, tiles);
            } else if let Some(tiles) = blocks.inventory_cube_tiles(root, id)? {
                // Chests draw as a cube of their inventory tiles, so the GUI can draw them in 3D.
                carried_tiles.insert(visual, tiles);
            }
        }
    }
    let mut model_sprites = BTreeMap::new();
    let mut model_sprites_by_state = BTreeMap::new();
    if let Some(world) = world {
        let model_keys: BTreeSet<_> = block_plan
            .keys()
            .map(|&visual| (visual, 0))
            .chain(compiled.item_visuals.iter().filter_map(|definition| {
                let assets::ItemVisualDefinitionRoute::BlockItem { block_visual } =
                    definition.route
                else {
                    return None;
                };
                let name = blocks?.name(block_visual)?;
                let metadata = super::block_entity::metadata_variant(name, definition.key.metadata);
                block_plan
                    .contains_key(&block_visual.0)
                    .then_some((block_visual.0, metadata))
            }))
            .collect();
        for (visual, metadata) in model_keys {
            let plan = &block_plan[&visual];
            let raster = if let Some(tiles) = carried_tiles.get(&visual) {
                Some((
                    model::Model::cube(
                        tiles.clone().map(|tile| tile.rgba8.to_vec().into()),
                        cube_blending(world, BlockVisualId(visual)),
                    )
                    .raster(),
                    None,
                ))
            } else if plan.is_ok()
                && !blocks
                    .and_then(|blocks| blocks.name(BlockVisualId(visual)))
                    .is_some_and(super::shapes::has_model)
            {
                continue;
            } else {
                model_raster(root, world, blocks, &compiled.geometries, visual, metadata)?
            };
            if let Some((raster, state)) = raster {
                let sprite = if let Some(state) = state {
                    *model_sprites_by_state.entry(state.0).or_insert_with(|| {
                        let sprite = sprites.len() as u32;
                        sprites.push(raster);
                        block_models.insert(sprite, state);
                        sprite
                    })
                } else {
                    insert_sprite(sprites, raster, &block_models)
                };
                model_sprites.insert((visual, metadata), sprite);
            }
        }
    }
    // Legacy sprite-only compilation accepts only its actually resolved keys.
    // World-aware compilation conservatively preflights every merged route.
    if world.is_some() {
        let block_count = block_plan
            .iter()
            .filter(|(visual, value)| value.is_ok() && !model_sprites.contains_key(&(**visual, 0)))
            .count();
        preflight(sprites, block_count, carried_tiles.len(), compiled)?;
    }
    let mut baked: BTreeMap<[u8; 32], Vec<(&cube::Cube<'_>, u32)>> = BTreeMap::new();
    let mut output_hashes: BTreeMap<[u8; 32], Vec<u32>> = BTreeMap::new();
    for (index, sprite) in sprites.iter().enumerate() {
        if !block_models.contains_key(&(index as u32)) {
            output_hashes
                .entry(Sha256::digest(&sprite.rgba8).into())
                .or_default()
                .push(index as u32);
        }
    }
    let mut block_sprites = BTreeMap::new();
    for (&visual, plan) in block_plan {
        if model_sprites.contains_key(&(visual, 0)) {
            continue;
        }
        let Ok(plan) = plan else {
            continue;
        };
        let sprite = if let Some((_, index)) = baked
            .get(&plan.digest())
            .into_iter()
            .flat_map(|bucket| bucket.iter())
            .find(|(previous, _)| plan.same_source(previous))
        {
            *index
        } else {
            let raster = plan.raster();
            let output: [u8; 32] = Sha256::digest(&raster.rgba8).into();
            // Digest collisions still compare full pixels and dimensions.
            let index = output_hashes
                .get(&output)
                .into_iter()
                .flat_map(|bucket| bucket.iter())
                .find(|&&index| sprites[index as usize] == raster)
                .copied()
                .unwrap_or_else(|| {
                    let index = sprites.len() as u32;
                    sprites.push(raster);
                    output_hashes.entry(output).or_default().push(index);
                    index
                });
            baked.entry(plan.digest()).or_default().push((plan, index));
            index
        };
        block_sprites.insert(visual, sprite);
    }
    let block_sheets = carried_tiles
        .iter()
        .filter_map(|(&visual, tiles)| {
            let sprite = compose_block_item_sheet(tiles)?;
            Some(IconBlockSheet {
                visual: BlockVisualId(visual),
                sprite: insert_sprite(sprites, sprite, &block_models),
            })
        })
        .collect();
    Ok(BakedBlocks {
        flat_sprites,
        model_sprites,
        block_sprites,
        block_sheets,
        block_models: block_models
            .into_iter()
            .map(|(sprite, visual)| IconBlockModel { sprite, visual })
            .collect(),
    })
}

/// Reuses thumbnail pixels only when the sprite is not bound to a native block model.
fn insert_sprite(
    sprites: &mut Vec<IconSprite>,
    sprite: IconSprite,
    block_models: &BTreeMap<u32, BlockVisualId>,
) -> u32 {
    sprites
        .iter()
        .enumerate()
        .find_map(|(index, known)| {
            (*known == sprite && !block_models.contains_key(&(index as u32))).then_some(index)
        })
        .unwrap_or_else(|| {
            sprites.push(sprite);
            sprites.len() - 1
        }) as u32
}

fn preflight(
    sprites: &[IconSprite],
    block_count: usize,
    sheet_count: usize,
    compiled: &CompiledEntityAssets,
) -> Result<(), AssetError> {
    let mut predicted_bytes = 96usize;
    for sprite in sprites {
        predicted_bytes = predicted_bytes
            .checked_add(4 + sprite.rgba8.len())
            .ok_or_else(|| cube::invalid("icon byte count overflow"))?;
    }
    if sprites.len() + block_count + sheet_count > assets::MAX_ICON_SPRITES {
        return Err(cube::invalid("merged icon sprite count exceeds bound"));
    }
    predicted_bytes = predicted_bytes
        .checked_add(block_count * (4 + cube::PIXEL_BYTES))
        .and_then(|bytes| {
            let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
            bytes.checked_add(sheet_count * (12 + usize::from(width) * usize::from(height) * 4))
        })
        .ok_or_else(|| cube::invalid("icon byte count overflow"))?;
    let mut predicted_entries = 0usize;
    for key in compiled
        .item_visuals
        .iter()
        .map(|visual| &visual.key)
        .chain(compiled.item_visual_aliases.iter().map(|alias| &alias.key))
    {
        if key.identifier.len() > assets::MAX_ICON_KEY_BYTES {
            return Err(cube::invalid("icon key exceeds bound"));
        }
        predicted_entries += 1;
        predicted_bytes = predicted_bytes
            .checked_add(10 + key.identifier.len())
            .ok_or_else(|| cube::invalid("icon byte count overflow"))?;
    }
    if predicted_entries > assets::MAX_ICON_ENTRIES
        || predicted_bytes > assets::MAX_ICON_CARRIER_BYTES
    {
        return Err(cube::invalid("merged icon carrier exceeds bound"));
    }
    Ok(())
}

/// A refused opaque thumbnail may still have model geometry or explicit carried faces. The world
/// state comes back when the thumbnail projects that state's GUI quads.
fn model_raster(
    root: &Path,
    world: &RuntimeAssets,
    blocks: Option<&IconBlocks>,
    geometries: &[EntityGeometry],
    visual: u32,
    metadata: u32,
) -> Result<Option<(IconSprite, Option<BlockVisualId>)>, AssetError> {
    let visual = BlockVisualId(visual);
    if let Some(blocks) = blocks
        && blocks.name(visual).is_some_and(super::shapes::has_model)
    {
        return Ok(super::shapes::raster(root, blocks, visual)?.map(|raster| (raster, None)));
    }
    let state = blocks.map_or(visual, |blocks| blocks.icon_state(visual));
    if let Ok(model) = model::Model::read(world, state) {
        return Ok(Some((model.raster(), Some(state))));
    }
    Ok(
        fallback_raster(root, world, blocks, geometries, visual, metadata)?
            .map(|raster| (raster, None)),
    )
}

/// Explicit inventory or carried faces, or a block entity's model, for a block without geometry.
fn fallback_raster(
    root: &Path,
    world: &RuntimeAssets,
    blocks: Option<&IconBlocks>,
    geometries: &[EntityGeometry],
    visual: BlockVisualId,
    metadata: u32,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(blocks) = blocks else {
        return Ok(None);
    };
    if let Some(tiles) = blocks.inventory_cube_tiles(root, visual)? {
        return Ok(Some(
            model::Model::cube(
                tiles.map(|tile| tile.rgba8.to_vec().into_boxed_slice()),
                [false; 6],
            )
            .raster(),
        ));
    }
    if let Some(name) = blocks.name(visual)
        && let Some(raster) = super::block_entity::raster(root, name, metadata, geometries)?
    {
        return Ok(Some(raster));
    }
    Ok(blocks.carried_tiles(root, visual)?.map(|tiles| {
        model::Model::cube(
            tiles.map(|tile| tile.rgba8.to_vec().into_boxed_slice()),
            cube_blending(world, visual),
        )
        .raster()
    }))
}

fn cube_blending(world: &RuntimeAssets, visual: BlockVisualId) -> [bool; 6] {
    let block = world.resolve(NetworkIdMode::Sequential, visual.0);
    assets::BlockFace::ALL.map(|face| {
        world.materials()[block.face(face).material_id() as usize].flags
            & assets::MATERIAL_FLAG_ALPHA_BLEND
            != 0
    })
}

#[cfg(test)]
mod block_entity_tests {
    use super::*;

    #[test]
    fn banner_metadata_bakes_sixteen_colored_models_without_the_sign_fallback() {
        let root = tempfile::tempdir().unwrap();
        for (path, bytes) in [
            ("entity/item.entity.json", br#"{"format_version":"1.10.0","minecraft:client_entity":{"description":{"identifier":"minecraft:item","geometry":{"default":"geometry.item"},"render_controllers":["controller.render.item"]}}}"#.as_slice()),
            ("models/entity/item.geo.json", br#"{"format_version":"1.21.0","minecraft:geometry":[{"description":{"identifier":"geometry.item"},"bones":[{"name":"root"}]}]}"#),
            ("animations/empty.json", br#"{"format_version":"1.8.0","animations":{}}"#),
            ("animation_controllers/empty.json", br#"{"format_version":"1.10.0","animation_controllers":{}}"#),
            ("render_controllers/item.json", br#"{"format_version":"1.8.0","render_controllers":{"controller.render.item":{"geometry":"Geometry.default"}}}"#),
            ("textures/item_texture.json", br#"{"texture_data":{"sign":{"textures":"textures/items/sign"}}}"#),
            ("textures/flipbook_textures.json", br#"[]"#),
            ("blocks.json", br#"{"format_version":[1,1,0]}"#),
            ("textures/terrain_texture.json", br#"{"texture_data":{}}"#),
        ] {
            let file = root.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, bytes).unwrap();
        }
        for (path, size, color) in [
            ("textures/entity/banner/banner_base.tga", 64, [255; 4]),
            ("textures/items/sign.png", 16, [0, 0, 255, 255]),
        ] {
            let file = root.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            image::RgbaImage::from_pixel(size, size, image::Rgba(color))
                .save(file)
                .unwrap();
        }
        let compiled = crate::compile_entity_assets(
            root.path(),
            include_bytes!("../../../../../assets/vanilla-source.json"),
        )
        .unwrap();
        let mut visual = None;
        for metadata in 0..16 {
            let definition = compiled
                .item_visuals
                .iter()
                .find(|definition| {
                    definition.key.identifier.as_ref() == "minecraft:banner"
                        && definition.key.metadata == metadata
                })
                .unwrap();
            let assets::ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route
            else {
                panic!("banner aux {metadata} must draw its model instead of a sign sprite");
            };
            assert_eq!(*visual.get_or_insert(block_visual), block_visual);
        }
        let visual = visual.unwrap().0;
        let blocks = IconBlocks::read(root.path()).unwrap();
        let world = RuntimeAssets::diagnostic();
        let mut sprites = Vec::new();
        let baked = run(
            root.path(),
            Some(&world),
            Some(&blocks),
            &BTreeMap::new(),
            &BTreeMap::from([(visual, Err(cube::Reject::Geometry))]),
            &compiled,
            &mut sprites,
        )
        .unwrap();
        let mut distinct = BTreeSet::new();
        for metadata in 0..16 {
            let sprite = baked.model_sprites[&(visual, metadata)];
            assert!(
                distinct.insert(sprite),
                "banner aux {metadata} needs its own color"
            );
            let color = assets::banner::color_rgb(i64::from(metadata));
            let colored = sprites[sprite as usize]
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[..3] == color && pixel[3] == 255)
                .count();
            assert!(
                colored > 80,
                "banner aux {metadata} has {colored} dyed cloth pixels"
            );
        }
    }

    #[test]
    fn entity_only_world_visuals_have_nonempty_inventory_models() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("textures")).unwrap();
        std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
        std::fs::write(
            root.path().join("blocks.json"),
            r#"{"format_version":[1,1,0]}"#,
        )
        .unwrap();
        std::fs::write(
            root.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{}}"#,
        )
        .unwrap();
        for (path, width, height, color) in [
            (
                "textures/entity/shulker/shulker_undyed",
                64,
                64,
                [160, 110, 160, 255],
            ),
            (
                "textures/entity/shulker/shulker_red",
                64,
                64,
                [190, 20, 20, 255],
            ),
            ("textures/blocks/conduit_base", 24, 12, [200, 170, 140, 255]),
            (
                "textures/blocks/decorated_pot_base",
                32,
                32,
                [180, 80, 30, 255],
            ),
            (
                "textures/blocks/decorated_pot_side",
                16,
                16,
                [130, 60, 20, 255],
            ),
            ("textures/blocks/lectern_base", 16, 16, [150, 90, 40, 255]),
            ("textures/blocks/lectern_sides", 16, 16, [130, 70, 30, 255]),
            ("textures/blocks/lectern_top", 16, 16, [180, 110, 60, 255]),
            ("textures/blocks/lectern_front", 16, 16, [170, 100, 50, 255]),
        ] {
            let file = root.path().join(format!("{path}.png"));
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            image::RgbaImage::from_pixel(width, height, image::Rgba(color))
                .save(file)
                .unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let world = RuntimeAssets::diagnostic();
        let raster = |name: &str| {
            let visual = records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap()
                .sequential_id;
            model_raster(root.path(), &world, Some(&blocks), &[], visual, 0)
                .unwrap()
                .map(|(icon, _)| icon)
        };
        let mut output = Vec::new();
        for name in [
            "minecraft:undyed_shulker_box",
            "minecraft:red_shulker_box",
            "minecraft:conduit",
            "minecraft:decorated_pot",
            "minecraft:lectern",
        ] {
            let icon =
                raster(name).expect("a placed entity-only visual still has an inventory model");
            assert_eq!([icon.width, icon.height], [model::SIDE as u16; 2]);
            let covered = icon
                .rgba8
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[3] != 0)
                .count();
            assert!(
                covered > 40 && covered < model::SIDE * model::SIDE,
                "{name}: {covered}"
            );
            output.push(icon);
        }
        assert_ne!(
            output[0], output[1],
            "shulker color selects its pack texture"
        );
        assert_ne!(
            output[2], output[3],
            "conduit and pot retain their separate model poses"
        );
        std::fs::remove_file(root.path().join("textures/blocks/conduit_base.png")).unwrap();
        assert!(
            raster("minecraft:conduit").is_none(),
            "an optional missing model texture remains an unresolved icon"
        );
    }

    #[test]
    fn box_model_heads_have_inventory_models_facing_the_south_side() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("textures")).unwrap();
        std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
        std::fs::write(
            root.path().join("blocks.json"),
            r#"{"format_version":[1,1,0]}"#,
        )
        .unwrap();
        std::fs::write(
            root.path().join("textures/terrain_texture.json"),
            r#"{"texture_data":{}}"#,
        )
        .unwrap();
        // Each head's texture has its own base color, a red face, and a clear hat region.
        const FACE: [u8; 4] = [255, 0, 0, 255];
        let heads = [
            (
                "minecraft:skeleton_skull",
                "skulls/skeleton",
                32,
                [200, 200, 190, 255],
            ),
            (
                "minecraft:wither_skeleton_skull",
                "skulls/wither_skeleton",
                32,
                [40, 40, 40, 255],
            ),
            (
                "minecraft:zombie_head",
                "skulls/zombie",
                32,
                [60, 140, 80, 255],
            ),
            ("minecraft:player_head", "steve", 64, [150, 110, 80, 255]),
            (
                "minecraft:creeper_head",
                "skulls/creeper",
                32,
                [80, 200, 60, 255],
            ),
        ];
        for (_, path, height, color) in heads {
            let mut texture = image::RgbaImage::from_pixel(64, height, image::Rgba(color));
            for (x, y, pixel) in texture.enumerate_pixels_mut() {
                if x >= 32 && y < 16 {
                    *pixel = image::Rgba([0; 4]);
                } else if (8..16).contains(&x) && (8..16).contains(&y) {
                    *pixel = image::Rgba(FACE);
                }
            }
            let file = root.path().join(format!("textures/entity/{path}.png"));
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            texture.save(file).unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let world = RuntimeAssets::diagnostic();
        let raster = |name: &str| {
            let visual = records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap()
                .sequential_id;
            model_raster(root.path(), &world, Some(&blocks), &[], visual, 0)
                .unwrap()
                .map(|(icon, state)| {
                    assert_eq!(state, None, "{name} is a thumbnail, not world quads");
                    icon
                })
        };
        let mut icons = Vec::new();
        for (name, ..) in heads {
            let icon = raster(name).expect("a placed-only head still has an inventory model");
            let pixels = icon.rgba8.as_chunks::<4>().0;
            let covered = pixels.iter().filter(|pixel| pixel[3] != 0).count();
            assert!(
                covered > model::SIDE * model::SIDE / 3,
                "{name}: the head fills the slot, {covered} pixels"
            );
            let face: Vec<usize> = (0..pixels.len())
                .filter(|&index| {
                    let [red, green, blue, alpha] = pixels[index];
                    red > 0 && green == 0 && blue == 0 && alpha == 255
                })
                .collect();
            assert!(face.len() > 20, "{name}: its face is drawn");
            assert!(
                face.iter()
                    .all(|index| index % model::SIDE >= model::SIDE / 2),
                "{name}: the face is on the south (right) side, like other block items' fronts"
            );
            icons.push(icon);
        }
        for (index, icon) in icons.iter().enumerate() {
            assert!(
                !icons[..index].contains(icon),
                "{}: each head draws its own texture",
                heads[index].0
            );
        }
        std::fs::remove_file(root.path().join("textures/entity/skulls/creeper.png")).unwrap();
        assert!(
            raster("minecraft:creeper_head").is_none(),
            "a missing head texture leaves the icon unresolved"
        );
    }

    #[test]
    fn placed_chest_visibility_does_not_remove_its_inventory_faces() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("textures/blocks")).unwrap();
        std::fs::write(root.path().join("textures/flipbook_textures.json"), "[]").unwrap();
        std::fs::write(
            root.path().join("blocks.json"),
            r#"{
            "format_version": [1, 1, 0],
            "chest": {"textures": "wood"},
            "copper_chest": {"textures": "copper"},
            "waxed_copper_chest": {"textures": "copper"}
        }"#,
        )
        .unwrap();
        std::fs::write(
            root.path().join("textures/terrain_texture.json"),
            r#"{
            "texture_name": "atlas.terrain", "texture_data": {
                "wood": {"textures": "textures/blocks/wood"},
                "copper": {"textures": "textures/blocks/copper"}
            }
        }"#,
        )
        .unwrap();
        for (name, color) in [
            ("wood", [160, 80, 20, 255]),
            ("copper", [30, 180, 180, 255]),
        ] {
            image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
                .save(root.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let world = RuntimeAssets::diagnostic();
        let mut rasters = Vec::new();
        for name in [
            "minecraft:chest",
            "minecraft:copper_chest",
            "minecraft:waxed_copper_chest",
        ] {
            let visual = records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap()
                .sequential_id;
            let (raster, state) = model_raster(root.path(), &world, Some(&blocks), &[], visual, 0)
                .unwrap()
                .expect("the inventory cube survives an entity-only placed visual");
            // Only template geometry is redrawn at runtime; these faces stay a thumbnail.
            assert_eq!(state, None);
            assert!(
                raster
                    .rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|pixel| pixel[3] == 255)
            );
            rasters.push(raster);
        }
        assert_ne!(rasters[0], rasters[1]);
        assert_eq!(rasters[1], rasters[2]);
    }
}

#[cfg(test)]
#[path = "bake/dried_ghast_tests.rs"]
mod dried_ghast_tests;

#[cfg(test)]
#[path = "bake/sensor_tests.rs"]
mod sensor_tests;

#[cfg(test)]
#[path = "bake/shrieker_tests.rs"]
mod shrieker_tests;
