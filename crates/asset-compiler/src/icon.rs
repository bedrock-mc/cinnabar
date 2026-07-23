//! Item-icon compiler: bakes the exact sprite pixels for every sprite-routed
//! item visual (and alias) the entity compilation resolves from the pinned
//! pack, deduplicated by raster source, into the bounded icon carrier.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use assets::{
    AssetError, IconEntry, IconSprite, ItemVisualDefinitionRoute, MAX_ICON_SIDE,
    encode_icon_catalog,
};
use sha2::{Digest, Sha256};

use crate::entity::compile_entity_assets;
use crate::image::decode_texture;

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
    pub alias_entries: usize,
    /// Vertical animation strips reduced to their first recorded frame.
    pub animation_strips: usize,
    /// Raster sources outside the flat-icon bounds, skipped and counted.
    pub skipped_oversized: usize,
}

pub fn compile_icon_assets(
    root: &Path,
    source_manifest: &[u8],
) -> Result<CompiledIconCarrier, AssetError> {
    let compiled = compile_entity_assets(root, source_manifest)?;
    let mut sprites: Vec<IconSprite> = Vec::new();
    let mut sprite_by_source: BTreeMap<u32, Option<u32>> = BTreeMap::new();
    let mut animation_strips = 0usize;
    let mut skipped_oversized = 0usize;
    let mut entries: Vec<IconEntry> = Vec::new();
    let mut sprite_visuals = 0usize;

    let mut sprite_for_source =
        |source_index: u32, sprites: &mut Vec<IconSprite>| -> Result<Option<u32>, AssetError> {
            if let Some(existing) = sprite_by_source.get(&source_index) {
                return Ok(*existing);
            }
            let source = &compiled.sources[source_index as usize];
            let decoded = decode_texture(&root.join(source.path.as_ref()), &source.path)?;
            let (width, height, rgba8) =
                if decoded.width <= MAX_ICON_SIDE && decoded.height <= MAX_ICON_SIDE {
                    (decoded.width, decoded.height, decoded.rgba8)
                } else if decoded.width <= MAX_ICON_SIDE
                    && decoded.height > decoded.width
                    && decoded.height % decoded.width.max(1) == 0
                {
                    // A vertical animation strip (compass, clock): the flat inventory
                    // icon is the strip's first recorded frame, never a guessed crop.
                    animation_strips += 1;
                    let frame_bytes = decoded.width as usize * decoded.width as usize * 4;
                    (
                        decoded.width,
                        decoded.width,
                        decoded.rgba8[..frame_bytes].to_vec().into_boxed_slice(),
                    )
                } else {
                    skipped_oversized += 1;
                    sprite_by_source.insert(source_index, None);
                    return Ok(None);
                };
            let index =
                u32::try_from(sprites.len()).map_err(|_| AssetError::InvalidCompiledAssets {
                    detail: "icon sprite count exceeds platform".into(),
                })?;
            sprites.push(IconSprite {
                width: u16::try_from(width).expect("bounded by MAX_ICON_SIDE"),
                height: u16::try_from(height).expect("bounded by MAX_ICON_SIDE"),
                rgba8: Arc::from(rgba8),
            });
            sprite_by_source.insert(source_index, Some(index));
            Ok(Some(index))
        };

    let mut visual_sprites: Vec<Option<u32>> = Vec::with_capacity(compiled.item_visuals.len());
    for visual in compiled.item_visuals.iter() {
        let sprite = match visual.route {
            ItemVisualDefinitionRoute::Sprite { texture } => {
                sprite_visuals += 1;
                sprite_for_source(texture.source, &mut sprites)?
            }
            ItemVisualDefinitionRoute::BlockItem { .. }
            | ItemVisualDefinitionRoute::EmptyHand
            | ItemVisualDefinitionRoute::Missing => None,
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

    let bytes = encode_icon_catalog(compiled.source_manifest_sha256, &sprites, &entries)?;
    Ok(CompiledIconCarrier {
        report: IconCompileReport {
            source_manifest_sha256: compiled.source_manifest_sha256,
            carrier_sha256: Sha256::digest(&bytes).into(),
            sprites: sprites.len(),
            entries: entries.len(),
            sprite_visuals,
            alias_entries,
            animation_strips,
            skipped_oversized,
        },
        bytes,
    })
}
