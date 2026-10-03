//! Item-icon compiler: bakes the exact sprite pixels for every sprite-routed
//! item visual (and alias) the entity compilation resolves from the pinned
//! pack, deduplicated by raster source, into the bounded icon carrier.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use assets::{
    AssetError, IconEntry, IconSprite, ItemVisualDefinitionRoute, MAX_ICON_BLOCK_SHEETS,
    MAX_ICON_SIDE, encode_icon_catalog_with_block_sheets,
};
use sha2::{Digest, Sha256};

use crate::entity::compile_entity_assets_with_report;

mod bake;
mod blocks;
mod carried;
mod cube;
mod model;
mod shield;

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
    let mut sprite_by_source: BTreeMap<u32, Option<u32>> = BTreeMap::new();
    let mut animation_strips = 0usize;
    let mut skipped_oversized = 0usize;
    let mut entries: Vec<IconEntry> = Vec::new();
    let mut sprite_visuals = 0usize;
    let mut model_item_visuals = 0usize;
    let mut shield_sprite = None;

    let mut sprite_for_source =
        |source_index: u32, sprites: &mut Vec<IconSprite>| -> Result<Option<u32>, AssetError> {
            if let Some(existing) = sprite_by_source.get(&source_index) {
                return Ok(*existing);
            }
            let source = &compiled.sources[source_index as usize];
            let decoded = sprite_source(root, source)?;
            let Some((sprite, strip)) = bounded_sprite(decoded) else {
                skipped_oversized += 1;
                sprite_by_source.insert(source_index, None);
                return Ok(None);
            };
            animation_strips += usize::from(strip);
            let index =
                u32::try_from(sprites.len()).map_err(|_| AssetError::InvalidCompiledAssets {
                    detail: "icon sprite count exceeds platform".into(),
                })?;
            sprites.push(sprite);
            sprite_by_source.insert(source_index, Some(index));
            Ok(Some(index))
        };

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
                    sprite_for_source(texture.source, &mut sprites)?
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
            let model_sprite = model_sprites.get(&block.0);
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

    let bytes = encode_icon_catalog_with_block_sheets(
        compiled.source_manifest_sha256,
        &sprites,
        &entries,
        &block_sheets,
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
            animation_strips,
            skipped_oversized,
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

/// Bounds a decoded texture to a flat icon: as-is within `MAX_ICON_SIDE`, else a vertical
/// animation strip's first frame (`true`); anything else is refused.
fn bounded_sprite(decoded: crate::image::DecodedTexture) -> Option<(IconSprite, bool)> {
    let (width, height, rgba8, strip) =
        if decoded.width <= MAX_ICON_SIDE && decoded.height <= MAX_ICON_SIDE {
            (decoded.width, decoded.height, decoded.rgba8, false)
        } else if decoded.width <= MAX_ICON_SIDE
            && decoded.height > decoded.width
            && decoded.height.is_multiple_of(decoded.width.max(1))
        {
            // A vertical animation strip (compass, clock): the flat inventory icon is the
            // strip's first recorded frame, never a guessed crop.
            let frame_bytes = decoded.width as usize * decoded.width as usize * 4;
            let frame = decoded.rgba8[..frame_bytes].to_vec().into_boxed_slice();
            (decoded.width, decoded.width, frame, true)
        } else {
            return None;
        };
    let sprite = IconSprite {
        width: u16::try_from(width).ok()?,
        height: u16::try_from(height).ok()?,
        rgba8: Arc::from(rgba8),
    };
    Some((sprite, strip))
}

/// A 3D inventory thumbnail of state `visual` of a session block overlay; `None` when it has no
/// drawable geometry or needs a biome tint.
#[must_use]
pub fn overlay_block_icon(overlay: &assets::BlockOverlay, visual: usize) -> Option<IconSprite> {
    model::Model::overlay(overlay, visual)
        .ok()
        .map(|model| model.raster())
}

/// Decodes the texture used by a compiled sprite source.
fn sprite_source(
    root: &Path,
    source: &assets::EntityAssetSource,
) -> Result<crate::image::DecodedTexture, AssetError> {
    let path = root.join(source.path.as_ref());
    let bytes = crate::entity::read_bounded_source(root, &path)?;
    if bytes.len() != source.source_bytes as usize
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != source.source_sha256
    {
        return Err(AssetError::InvalidCompiledAssets {
            detail: "icon source changed after entity compilation".into(),
        });
    }
    crate::image::decode_texture_bytes(&path, &source.path, &bytes)
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_sprite_rereads_reject_changed_source_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sprite.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1; 4]))
            .save(&path)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let source = assets::EntityAssetSource {
            path: "sprite.png".into(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(bytes).into(),
        };
        image::RgbaImage::from_pixel(2, 1, image::Rgba([2; 4]))
            .save(&path)
            .unwrap();
        assert!(sprite_source(root.path(), &source).is_err());
    }
}
