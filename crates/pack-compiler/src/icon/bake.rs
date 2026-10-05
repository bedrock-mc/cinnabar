//! Bounded block-thumbnail and carried-sheet baking, sharing colored face
//! pixels between inventory thumbnails and runtime held-block geometry.

use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, BlockVisualId, CompiledEntityAssets, IconBlockSheet, IconSprite, NetworkIdMode,
    RuntimeAssets, compose_block_item_sheet,
};
use sha2::{Digest, Sha256};

use super::{blocks::IconBlocks, cube, model};

pub(super) struct BakedBlocks {
    pub flat_sprites: BTreeMap<u32, u32>,
    pub model_sprites: BTreeMap<u32, u32>,
    pub block_sprites: BTreeMap<u32, u32>,
    pub block_sheets: Vec<IconBlockSheet>,
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
    let mut flat_by_path: BTreeMap<Box<str>, Option<u32>> = BTreeMap::new();
    let mut flat_sprites = BTreeMap::new();
    for (&visual, path) in flat_plan {
        let Some(path) = path else {
            continue;
        };
        let sprite = match flat_by_path.get(path) {
            Some(existing) => *existing,
            None => {
                let sprite =
                    IconBlocks::sprite(root, path)?.map(|sprite| insert_sprite(sprites, sprite));
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
            if blocks.is_carried_cube(world, id)
                && let Some(tiles) = blocks.carried_cube_tiles(root, id)?
            {
                carried_tiles.insert(visual, tiles);
            }
        }
    }
    let mut model_sprites = BTreeMap::new();
    if let Some(world) = world {
        for (&visual, plan) in block_plan {
            let raster = if let Some(tiles) = carried_tiles.get(&visual) {
                Some(
                    model::Model::cube(
                        tiles.clone().map(|tile| tile.rgba8.to_vec().into()),
                        cube_blending(world, BlockVisualId(visual)),
                    )
                    .raster(),
                )
            } else if plan.is_ok() {
                continue;
            } else {
                model_raster(root, world, blocks, visual)?
            };
            if let Some(raster) = raster {
                model_sprites.insert(visual, insert_sprite(sprites, raster));
            }
        }
    }
    // Legacy sprite-only compilation accepts only its actually resolved keys.
    // World-aware compilation conservatively preflights every merged route.
    if world.is_some() {
        let block_count = block_plan
            .iter()
            .filter(|(visual, value)| value.is_ok() && !model_sprites.contains_key(visual))
            .count();
        preflight(sprites, block_count, carried_tiles.len(), compiled)?;
    }
    let mut baked: BTreeMap<[u8; 32], Vec<(&cube::Cube<'_>, u32)>> = BTreeMap::new();
    let mut output_hashes: BTreeMap<[u8; 32], Vec<u32>> = BTreeMap::new();
    for (index, sprite) in sprites.iter().enumerate() {
        output_hashes
            .entry(Sha256::digest(&sprite.rgba8).into())
            .or_default()
            .push(index as u32);
    }
    let mut block_sprites = BTreeMap::new();
    for (&visual, plan) in block_plan {
        if model_sprites.contains_key(&visual) {
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
                sprite: insert_sprite(sprites, sprite),
            })
        })
        .collect();
    Ok(BakedBlocks {
        flat_sprites,
        model_sprites,
        block_sprites,
        block_sheets,
    })
}

fn insert_sprite(sprites: &mut Vec<IconSprite>, sprite: IconSprite) -> u32 {
    sprites
        .iter()
        .position(|known| *known == sprite)
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

/// A refused opaque thumbnail may still have model geometry or explicit carried faces.
fn model_raster(
    root: &Path,
    world: &RuntimeAssets,
    blocks: Option<&IconBlocks>,
    visual: u32,
) -> Result<Option<IconSprite>, AssetError> {
    let visual = BlockVisualId(visual);
    let state = blocks.map_or(visual, |blocks| blocks.icon_state(visual));
    if let Ok(model) = model::Model::read(world, state) {
        return Ok(Some(model.raster()));
    }
    let Some(blocks) = blocks else {
        return Ok(None);
    };
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
