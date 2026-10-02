//! Session icons for server-defined items: the stack's item_texture.json, and custom block
//! items drawn as their block.

use std::{collections::HashSet, sync::Arc};

mod catalog;
mod vanilla;
pub(crate) use vanilla::set_vanilla_item_paths;

use resource_pack::LayeredPackView;

use super::resource_packs::{DecodedTexture, decode_pack_texture};
use crate::ui_runtime::presentation::{MAX_SESSION_ICON_SIDE, SessionIcon, SessionIcons};

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
    let block_rendered = block_icons
        .icons
        .iter()
        .map(|icon| Arc::clone(&icon.identifier))
        .chain(
            block_icons
                .misses
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
    let mut misses = std::collections::HashMap::new();
    for (identifier, reason) in block_icons.misses {
        if misses.len() < MAX_SESSION_ICONS {
            misses.insert(identifier, reason);
        }
    }
    let explicit_count = explicit.len();
    let mut block_icons = block_icons.icons.into_iter();
    for (index, (identifier, key)) in explicit.into_iter().chain(guessed).enumerate() {
        // Block items rank after explicit icons and before short-name guesses.
        if index == explicit_count {
            icons.extend(
                block_icons
                    .by_ref()
                    .take(MAX_SESSION_ICONS.saturating_sub(icons.len())),
            );
        }
        if icons.len() >= MAX_SESSION_ICONS {
            break;
        }
        match resolve_key(view, &paths, key) {
            Ok(textures) => {
                for (metadata, texture) in textures {
                    if icons.len() >= MAX_SESSION_ICONS {
                        break;
                    }
                    let mut sprite = icon(Arc::clone(identifier), first_frame(texture));
                    sprite.metadata = metadata;
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
    icons.extend(block_icons.take(MAX_SESSION_ICONS.saturating_sub(icons.len())));
    vanilla::append(view, &mut icons);
    (!icons.is_empty() || !misses.is_empty()).then(|| Arc::new(SessionIcons { icons, misses }))
}

/// Custom block item thumbnails, and why a block item has none.
#[derive(Default)]
pub(super) struct BlockIcons {
    pub(super) icons: Vec<SessionIcon>,
    pub(super) misses: Vec<(Arc<str>, Box<str>)>,
}

/// Pairs each registry item that draws as a custom block with that block: the block's own
/// item (a `BlockItem`, which vanilla always renders as its block), or a `block_placer` item
/// declaring no icon of its own.
pub(super) fn custom_block_items(
    game_data: &protocol::GameData,
    blocks: &protocol::CustomBlocks,
) -> Box<[(Arc<str>, Arc<str>)]> {
    let names = blocks
        .blocks
        .iter()
        .map(|block| Arc::clone(&block.name))
        .collect::<HashSet<_>>();
    if names.is_empty() {
        return Box::new([]);
    }
    let components = protocol::item_components(game_data);
    let mut pairs = game_data
        .item_registry
        .item_data
        .iter()
        .filter_map(|item| names.get(item.item_name.as_str()).cloned())
        .map(|block| (Arc::clone(&block), block))
        .collect::<Vec<_>>();
    for (identifier, facts) in components.iter() {
        if facts.icon.is_some() || names.contains(identifier) {
            continue;
        }
        if let Some(block) = facts
            .block_placer
            .as_deref()
            .and_then(|block| names.get(block))
        {
            pairs.push((Arc::clone(identifier), Arc::clone(block)));
        }
    }
    pairs.into_boxed_slice()
}

/// Thumbnails of each block item's block in its default (first) state, whose overlay index is
/// the block's first palette state, or first `hashed_states` entry in a hashed session.
pub(super) fn custom_block_icons(
    overlay: &assets::BlockOverlay,
    blocks: &protocol::CustomBlocks,
    hashed: bool,
    block_items: &[(Arc<str>, Arc<str>)],
) -> BlockIcons {
    let mut first_state = std::collections::HashMap::new();
    let mut offset = 0usize;
    for block in blocks.blocks.iter() {
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
    let mut result = BlockIcons::default();
    for (identifier, block) in block_items.iter().take(MAX_SESSION_ICONS) {
        let icon = first_state
            .get(block)
            .and_then(|&visual| asset_compiler::overlay_block_icon(overlay, visual));
        match icon {
            Some(sprite) => result.icons.push(SessionIcon {
                identifier: Arc::clone(identifier),
                metadata: 0,
                width: u32::from(sprite.width),
                height: u32::from(sprite.height),
                rgba8: sprite.rgba8.to_vec().into_boxed_slice(),
            }),
            None => result.misses.push((
                Arc::clone(identifier),
                format!("custom block {block} has no drawable default-state visual").into(),
            )),
        }
    }
    result
}

/// The image for icon `key`: the catalog's path, else `textures/items/<key>`.
fn resolve_key(
    view: &LayeredPackView,
    paths: &std::collections::HashMap<String, Vec<String>>,
    key: &str,
) -> Result<Vec<(u32, DecodedTexture)>, String> {
    let mut tried = Vec::new();
    if let Some(variants) = paths.get(key) {
        let textures: Vec<_> = variants
            .iter()
            .enumerate()
            .filter_map(|(metadata, path)| {
                decode_pack_texture(view, path).map(|texture| (metadata as u32, texture))
            })
            .collect();
        if !textures.is_empty() {
            return Ok(textures);
        }
        tried.push(format!("catalog key {key} has no readable images"));
    } else {
        tried.push(format!("key '{key}' not in the merged item_texture.json"));
    }
    let bare = key.rsplit_once(':').map_or(key, |(_, name)| name);
    for path in [
        format!("textures/items/{key}"),
        format!("textures/items/{bare}"),
    ] {
        if let Some(texture) = decode_pack_texture(view, &path) {
            return Ok(vec![(0, texture)]);
        }
    }
    tried.push(format!("no textures/items/{bare}"));
    Err(tried.join("; "))
}

/// Vertical strips animate in vanilla; the static icon is their first frame.
fn first_frame(texture: DecodedTexture) -> DecodedTexture {
    let side = texture.width;
    if texture.height <= side || !texture.height.is_multiple_of(side) {
        return texture;
    }
    let bytes = (side * side * 4) as usize;
    DecodedTexture {
        width: side,
        height: side,
        rgba8: texture.rgba8[..bytes].into(),
    }
}

/// Keeps icons up to the page limit as-is and reduces larger ones by
/// nearest-neighbour sampling, preserving aspect.
fn icon(identifier: Arc<str>, texture: DecodedTexture) -> SessionIcon {
    let longest = texture.width.max(texture.height);
    if longest <= MAX_SESSION_ICON_SIDE {
        return SessionIcon {
            identifier,
            metadata: 0,
            width: texture.width,
            height: texture.height,
            rgba8: texture.rgba8,
        };
    }
    let scale = |side: u32| (side * MAX_SESSION_ICON_SIDE / longest).max(1);
    let (width, height) = (scale(texture.width), scale(texture.height));
    let mut rgba8 = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let source_y = y * texture.height / height;
        for x in 0..width {
            let source_x = x * texture.width / width;
            let offset = ((source_y * texture.width + source_x) * 4) as usize;
            rgba8.extend_from_slice(&texture.rgba8[offset..offset + 4]);
        }
    }
    SessionIcon {
        identifier,
        metadata: 0,
        width,
        height,
        rgba8: rgba8.into(),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod capture_tests;
