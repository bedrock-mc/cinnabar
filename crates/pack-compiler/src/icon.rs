//! Item-icon compiler: bakes the exact sprite pixels for every sprite-routed
//! item visual (and alias) the entity compilation resolves from the pinned
//! pack, deduplicated by raster source, into the bounded icon carrier.

use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, IconEntry, IconSprite, ItemVisualDefinitionRoute, MAX_ICON_BLOCK_SHEETS,
    encode_icon_catalog_with_blocks,
};
use sha2::{Digest, Sha256};

use crate::entity::compile_entity_assets_with_report;

mod bake;
mod block_entity;
mod blocks;
mod carried;
mod cube;
mod model;
mod shield;
mod sprites;

#[cfg(test)]
mod tests;

#[derive(Debug)]
pub struct CompiledIconCarrier {
    pub bytes: Vec<u8>,
    pub report: IconCompileReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IconCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub sprites: usize,
    pub entries: usize,
    pub sprite_visuals: usize,
    /// Native GUI model icons, not raw model UV sheets.
    pub model_item_visuals: usize,
    pub alias_entries: usize,
    /// Vertical animation strips reduced to their first recorded frame.
    pub animation_strips: usize,
    /// Raster sources outside the flat-icon bounds, skipped and counted.
    pub skipped_oversized: usize,
    pub block_visuals: usize,
    /// Block items drawn as their flat carried texture.
    pub flat_block_visuals: usize,
    /// Non-cube 3D block items drawn from their isolated world template.
    pub model_block_visuals: usize,
    /// Carried six-face sheets reused for first-person and inventory rendering.
    pub carried_block_sheets: usize,
    pub skipped_blocks: usize,
    /// Item identifiers of block items left without an icon.
    pub unresolved_block_items: Vec<Box<str>>,
    /// Geometry, material, texture, and alpha refusals in that order.
    pub block_refusals: [usize; 4],
    pub block_registry_sha256: Option<[u8; 32]>,
    pub block_policy: Option<&'static str>,
}

pub fn compile_icon_assets(
    root: &Path,
    source_manifest: &[u8],
) -> Result<CompiledIconCarrier, AssetError> {
    compile(root, source_manifest, None)
}

/// Adds bounded ordinary opaque-cube thumbnails after all original sprites.
/// Other block presentation remains unavailable; side shading is provisional.
pub fn compile_icon_assets_with_blocks(
    root: &Path,
    source_manifest: &[u8],
    world: &assets::RuntimeAssets,
) -> Result<CompiledIconCarrier, AssetError> {
    compile(root, source_manifest, Some(world))
}

fn compile(
    root: &Path,
    source_manifest: &[u8],
    world: Option<&assets::RuntimeAssets>,
) -> Result<CompiledIconCarrier, AssetError> {
    let compilation = compile_entity_assets_with_report(root, source_manifest)?;
    let shield_icon = shield::compile(root, &compilation)?;
    let compiled = compilation.assets;
    if let Some(world) = world {
        cube::validate_world(
            world,
            compiled.source_manifest_sha256,
            compiled.block_visual_count as usize,
        )?;
    }
    let icon_blocks = world.map(|_| blocks::IconBlocks::read(root)).transpose()?;
    let mut block_plan = BTreeMap::new();
    let mut flat_plan = BTreeMap::new();
    if let (Some(world), Some(flat)) = (world, icon_blocks.as_ref()) {
        for visual in compiled.item_visuals.iter() {
            let ItemVisualDefinitionRoute::BlockItem {
                block_visual: block,
            } = visual.route
            else {
                continue;
            };
            if flat.is_flat(world, block) {
                flat_plan
                    .entry(block.0)
                    .or_insert_with(|| flat.texture_path(block));
            } else if !block_plan.contains_key(&block.0) {
                if block_plan.len() == MAX_ICON_BLOCK_SHEETS {
                    return Err(cube::invalid("block icon route count exceeds bound"));
                }
                block_plan.insert(block.0, cube::Cube::read(world, block));
            }
        }
    }
    let mut sprites: Vec<IconSprite> = Vec::new();
    let mut sprite_baker = sprites::SpriteBaker::default();
    let mut entries: Vec<IconEntry> = Vec::new();
    let mut sprite_visuals = 0usize;
    let mut model_item_visuals = 0usize;
    let mut shield_sprite = None;

    let mut visual_sprites: Vec<Option<u32>> = Vec::with_capacity(compiled.item_visuals.len());
    for visual in compiled.item_visuals.iter() {
        let sprite = if visual.key.identifier.as_ref() == shield::IDENTIFIER {
            if let Some(icon) = shield_icon.as_ref() {
                model_item_visuals += 1;
                Some(*shield_sprite.get_or_insert_with(|| {
                    let index = sprites.len() as u32;
                    sprites.push(icon.clone());
                    index
                }))
            } else {
                None
            }
        } else {
            match visual.route {
                ItemVisualDefinitionRoute::Sprite { texture } => {
                    sprite_visuals += 1;
                    sprite_baker.bake(root, &compiled, texture.source, &mut sprites)?
                }
                ItemVisualDefinitionRoute::BlockItem { .. }
                | ItemVisualDefinitionRoute::EmptyHand
                | ItemVisualDefinitionRoute::Missing => None,
            }
        };
        if let Some(sprite) = sprite {
            entries.push(IconEntry {
                identifier: visual.key.identifier.clone(),
                metadata: visual.key.metadata,
                sprite,
            });
        }
        visual_sprites.push(sprite);
    }
    let bake::BakedBlocks {
        flat_sprites,
        model_sprites,
        block_sprites,
        block_sheets,
        block_models,
    } = bake::run(
        root,
        world,
        icon_blocks.as_ref(),
        &flat_plan,
        &block_plan,
        &compiled,
        &mut sprites,
    )?;
    let mut block_visuals = 0usize;
    let mut flat_block_visuals = 0usize;
    let mut model_block_visuals = 0usize;
    let mut skipped_blocks = 0usize;
    let mut block_refusals = [0usize; 4];
    let mut unresolved_block_items = Vec::new();
    for (index, visual) in compiled.item_visuals.iter().enumerate() {
        if let ItemVisualDefinitionRoute::BlockItem {
            block_visual: block,
        } = visual.route
        {
            let flat_sprite = flat_sprites.get(&block.0);
            let model_sprite = model_sprites
                .get(&(block.0, visual.key.metadata))
                .or_else(|| model_sprites.get(&(block.0, 0)));
            flat_block_visuals += usize::from(flat_sprite.is_some());
            model_block_visuals += usize::from(model_sprite.is_some());
            if let Some(&sprite) = flat_sprite
                .or(model_sprite)
                .or_else(|| block_sprites.get(&block.0))
            {
                block_visuals += 1;
                visual_sprites[index] = Some(sprite);
                entries.push(IconEntry {
                    identifier: visual.key.identifier.clone(),
                    metadata: visual.key.metadata,
                    sprite,
                });
            } else if world.is_some() {
                skipped_blocks += 1;
                unresolved_block_items.push(visual.key.identifier.clone());
                if let Some(Err(reason)) = block_plan.get(&block.0) {
                    block_refusals[*reason as usize] += 1;
                }
            }
        }
    }
    let mut alias_entries = 0usize;
    for alias in compiled.item_visual_aliases.iter() {
        if let Some(sprite) = visual_sprites[alias.visual.0 as usize] {
            alias_entries += 1;
            entries.push(IconEntry {
                identifier: alias.key.identifier.clone(),
                metadata: alias.key.metadata,
                sprite,
            });
        }
    }
    entries.sort_by(|a, b| {
        (a.identifier.as_ref(), a.metadata).cmp(&(b.identifier.as_ref(), b.metadata))
    });

    let bytes = encode_icon_catalog_with_blocks(
        compiled.source_manifest_sha256,
        &sprites,
        &entries,
        &block_sheets,
        &block_models,
    )?;
    Ok(CompiledIconCarrier {
        report: IconCompileReport {
            source_manifest_sha256: compiled.source_manifest_sha256,
            carrier_sha256: Sha256::digest(&bytes).into(),
            sprites: sprites.len(),
            entries: entries.len(),
            sprite_visuals,
            model_item_visuals,
            alias_entries,
            animation_strips: sprite_baker.animation_strips,
            skipped_oversized: sprite_baker.skipped_oversized,
            block_visuals,
            flat_block_visuals,
            model_block_visuals,
            carried_block_sheets: block_sheets.len(),
            skipped_blocks,
            unresolved_block_items,
            block_refusals,
            block_registry_sha256: world.map(|world| world.provenance().block_registry_sha256),
            block_policy: world.map(|_| cube::POLICY),
        },
        bytes,
    })
}

/// A 3D inventory thumbnail of state `visual` of a session block overlay; `None` when it has no
/// drawable geometry or needs a biome tint.
#[must_use]
pub fn overlay_block_icon(overlay: &assets::BlockOverlay, visual: usize) -> Option<IconSprite> {
    model::Model::overlay(overlay, visual)
        .ok()
        .map(|model| model.raster())
}
