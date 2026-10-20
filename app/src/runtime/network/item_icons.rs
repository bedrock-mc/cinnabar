//! Session icons for server-defined items: the stack's item_texture.json, and custom block
//! items drawn as their block.

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

mod catalog;
mod images;
mod tints;
use images::{icon, resolve_key};
pub(super) use tints::carried_grass_tint;
pub(super) mod vanilla;
pub(super) mod vanilla_blocks;
pub(crate) use vanilla::{install_vanilla_item_assets, vanilla_item_paths_installed};

use resource_pack::LayeredPackView;

use assets::texture::policy::MAX_SUBSCRIBER_BYTES;
use client_ui::ui_runtime::presentation::{SessionIcon, SessionIcons};
use render_model::equipment::blocks::overlay_item;

/// One icon per registry item, the most a session can name.
const MAX_SESSION_ICONS: usize = protocol::MAX_ITEM_REGISTRY_ENTRIES;

/// Resolves each `(identifier, icon key)` through the item texture catalog merged across the
/// whole stack, then `textures/items/<key>`; misses are recorded with their reason. Items in
/// `block_icons` (successes and misses) keep their block rendering instead.
pub(super) fn compile_session_icons(
    view: &LayeredPackView,
    icon_keys: &[(Arc<str>, Arc<str>)],
    block_icons: BlockIcons,
) -> Option<Arc<SessionIcons>> {
    let BlockIcons {
        icons: block_icons,
        block_sheets,
        block_material_flags,
        block_models,
        misses: block_misses,
    } = block_icons;
    let block_rendered = block_icons
        .iter()
        .map(|icon| Arc::clone(&icon.identifier))
        .chain(
            block_sheets
                .iter()
                .map(|sheet| Arc::clone(&sheet.identifier)),
        )
        .chain(
            block_misses
                .iter()
                .map(|(identifier, _)| Arc::clone(identifier)),
        )
        .collect::<HashSet<_>>();
    let paths = catalog::paths(view);
    let icon_keys = catalog::icon_keys(view, icon_keys);
    // Explicit icon components outrank short-name guesses when the icon cap bites.
    let short_name = |identifier: &str| {
        identifier
            .rsplit_once(':')
            .map_or(identifier, |(_, name)| name)
            .to_owned()
    };
    let (guessed, explicit): (Vec<_>, Vec<_>) = icon_keys
        .iter()
        .filter(|(identifier, _)| !block_rendered.contains(identifier))
        .partition(|(identifier, key)| key.as_ref() == short_name(identifier));
    let mut icons = Vec::new();
    let mut retained_bytes = 0;
    let mut bounded_sheets = Vec::new();
    append_block_icons(
        &mut bounded_sheets,
        block_sheets.into_iter(),
        &mut retained_bytes,
    );
    let block_sheets = bounded_sheets;
    let mut misses = std::collections::HashMap::new();
    for (identifier, reason) in block_misses {
        if misses.len() < MAX_SESSION_ICONS {
            misses.insert(identifier, reason);
        }
    }
    let explicit_count = explicit.len();
    let mut block_icons = block_icons.into_iter();
    for (index, (identifier, key)) in explicit.into_iter().chain(guessed).enumerate() {
        // Block items rank after explicit icons and before short-name guesses.
        if index == explicit_count {
            append_block_icons(&mut icons, block_icons.by_ref(), &mut retained_bytes);
        }
        if icons.len() >= MAX_SESSION_ICONS {
            break;
        }
        match resolve_key(
            view,
            &paths,
            key,
            MAX_SESSION_ICONS.saturating_sub(icons.len()),
            MAX_SUBSCRIBER_BYTES.saturating_sub(retained_bytes),
            !identifier.starts_with("minecraft:") || key.as_ref() != short_name(identifier),
        ) {
            Ok(textures) => {
                for (metadata, texture) in textures {
                    if icons.len() >= MAX_SESSION_ICONS {
                        break;
                    }
                    let mut sprite = icon(Arc::clone(identifier), texture);
                    sprite.metadata = metadata;
                    retained_bytes += sprite.rgba8.len();
                    icons.push(sprite);
                }
            }
            Err(reason) => {
                // A short-name guess that misses is the normal vanilla-item case.
                if key.as_ref() != short_name(identifier) && misses.len() < MAX_SESSION_ICONS {
                    misses.insert(Arc::clone(identifier), reason.into_boxed_str());
                }
            }
        }
    }
    append_block_icons(&mut icons, block_icons, &mut retained_bytes);
    vanilla::append(view, &mut icons, &mut retained_bytes);
    (!icons.is_empty() || !block_sheets.is_empty() || !misses.is_empty()).then(|| {
        Arc::new(SessionIcons {
            icons,
            block_material_flags,
            block_models: block_models
                .into_iter()
                .filter(|(identifier, _)| {
                    block_sheets
                        .iter()
                        .any(|sheet| sheet.identifier == *identifier)
                })
                .collect(),
            block_sheets,
            misses,
        })
    })
}

/// Preserves block icon priority without exceeding the shared retained pixel budget.
fn append_block_icons(
    icons: &mut Vec<SessionIcon>,
    blocks: impl Iterator<Item = SessionIcon>,
    retained_bytes: &mut usize,
) {
    for icon in blocks {
        if icons.len() >= MAX_SESSION_ICONS {
            break;
        }
        if icon.rgba8.len() <= MAX_SUBSCRIBER_BYTES.saturating_sub(*retained_bytes) {
            *retained_bytes += icon.rgba8.len();
            icons.push(icon);
        }
    }
}

/// Custom block item thumbnails, held models and material sheets, and why a block item has no thumbnail.
#[derive(Default)]
pub(super) struct BlockIcons {
    pub(super) icons: Vec<SessionIcon>,
    pub(super) block_sheets: Vec<SessionIcon>,
    pub(super) block_models: BTreeMap<Arc<str>, Arc<[render_model::ActorRigVertex]>>,
    pub(super) block_material_flags: BTreeMap<Arc<str>, u32>,
    pub(super) misses: Vec<(Arc<str>, Box<str>)>,
}

#[cfg(test)]
use pack_runtime::custom_block_items;

/// Thumbnails of each block item's block in its default (first) state, whose overlay index is
/// the block's first palette state, or first `hashed_states` entry in a hashed session. A plain
/// cube state also yields the six-face sheet hands draw as that cube.
#[cfg(test)]
pub(super) fn custom_block_icons(
    overlay: &assets::BlockOverlay,
    blocks: &protocol::CustomBlocks,
    hashed: bool,
    block_items: &[(Arc<str>, Arc<str>)],
    base: Option<&assets::RuntimeAssets>,
) -> BlockIcons {
    custom_block_icons_with_tint(overlay, blocks, hashed, block_items, base, None)
}

/// Resolves supported carried tint colours independently of placed biome shading.
pub(super) fn custom_block_icons_with_tint(
    overlay: &assets::BlockOverlay,
    blocks: &protocol::CustomBlocks,
    hashed: bool,
    block_items: &[(Arc<str>, Arc<str>)],
    base: Option<&assets::RuntimeAssets>,
    grass: Option<[u8; 3]>,
) -> BlockIcons {
    let mut first_state = std::collections::HashMap::new();
    let mut offset = 0usize;
    for block in blocks.blocks.iter() {
        if block.visual.materialization_disabled {
            break;
        }
        let count = if hashed {
            block.hashed_states().len()
        } else {
            block.state_count as usize
        };
        if count > 0 {
            first_state.insert(Arc::clone(&block.name), offset);
        }
        offset = offset.saturating_add(count);
    }
    let session_icon = |identifier: &Arc<str>, sprite: assets::IconSprite| SessionIcon {
        identifier: Arc::clone(identifier),
        metadata: 0,
        width: u32::from(sprite.width),
        height: u32::from(sprite.height),
        rgba8: sprite.rgba8.to_vec().into_boxed_slice(),
    };
    let mut result = BlockIcons::default();
    let mut model_bytes = 0usize;
    for (identifier, block) in block_items.iter().take(MAX_SESSION_ICONS) {
        let visual = first_state.get(block).copied();
        if let Some(visual) = visual
            && let Some((sheet, model)) = overlay_item(overlay, visual, base, &|flags| {
                (flags & assets::MATERIAL_FLAG_TINT_MASK == assets::MATERIAL_FLAG_GRASS_TINT)
                    .then_some(grass)
                    .flatten()
            })
            .filter(|(_, model)| {
                let bytes = model.as_ref().map_or(0, |model| {
                    model
                        .len()
                        .saturating_mul(std::mem::size_of::<render_model::ActorRigVertex>())
                });
                if bytes > MAX_SUBSCRIBER_BYTES.saturating_sub(model_bytes) {
                    return false;
                }
                model_bytes += bytes;
                true
            })
        {
            let block = overlay.visuals[visual];
            let mut material_flags = block.faces.iter().fold(0, |flags, material| {
                flags | overlay.materials[*material as usize].flags
            });
            if model.is_some() {
                for part in
                    assets::model_template_parts(&overlay.model_templates, block.model_template)
                        .into_iter()
                        .flatten()
                {
                    for quad in
                        &overlay.model_quads[part.quad_start as usize..][..part.quad_count as usize]
                    {
                        material_flags |= overlay.materials[quad.material as usize].flags;
                    }
                }
            }
            result
                .block_material_flags
                .insert(Arc::clone(identifier), material_flags);
            if let Some(model) = model {
                result
                    .block_models
                    .insert(Arc::clone(identifier), model.into());
            }
            result.block_sheets.push(session_icon(identifier, sheet));
        }
        match visual.and_then(|visual| {
            pack_entity_compiler::overlay_block_icon_with_tint(overlay, visual, base, &|flags| {
                (flags & assets::MATERIAL_FLAG_TINT_MASK == assets::MATERIAL_FLAG_GRASS_TINT)
                    .then_some(grass)
                    .flatten()
            })
        }) {
            Some(sprite) => result.icons.push(session_icon(identifier, sprite)),
            None => result.misses.push((
                Arc::clone(identifier),
                format!("custom block {block} has no drawable default-state visual").into(),
            )),
        }
    }
    result
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod capture_tests;

#[cfg(test)]
mod startup_snapshot_tests;

#[cfg(test)]
mod startup_fixture;

#[cfg(test)]
mod startup_held_tests;
