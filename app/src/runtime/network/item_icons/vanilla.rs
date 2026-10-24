//! Base-game rasters behind pack references: item carrier sprites and their source routes, and
//! world carrier terrain layers by source image path.
use super::{MAX_SESSION_ICONS, icon};
use assets::{
    ItemVisualDefinitionRoute, RuntimeAssets, RuntimeEntityAssets, material_keys::MaterialKeys,
};
use client_ui::ui_runtime::presentation::SessionIcon;
use pack_runtime::pack_textures::DecodedTexture;
use resource_pack::LayeredPackView;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

struct Route {
    identifier: Arc<str>,
    metadata: u32,
    path: String,
    variant: u32,
    aliases: Vec<String>,
}
pub(in crate::runtime::network) static CACHE_IDENTITY: OnceLock<Option<([u8; 32], [u8; 32])>> =
    OnceLock::new();

static PATHS: OnceLock<Vec<Route>> = OnceLock::new();

/// Whether the carrier's item routes are installed; once installed they never change.
pub(crate) fn vanilla_item_paths_installed() -> bool {
    PATHS.get().is_some()
}

/// Installs the item routes and matching base artwork before pack compilation.
pub(crate) fn install_vanilla_item_assets(
    entities: &RuntimeEntityAssets,
    icons: &assets::RuntimeIconCatalog,
) {
    set_vanilla_item_paths(entities);
    set_vanilla_item_sprites(entities, icons);
    let _ = CACHE_IDENTITY.set(
        entities
            .carrier_identity()
            .map(|identity| (identity, icons.carrier_identity())),
    );
}

/// Retains source routes once, so menus can apply packs before any item registry arrives.
fn set_vanilla_item_paths(entities: &RuntimeEntityAssets) {
    super::vanilla_blocks::install(entities);
    let mut aliases_by_source = HashMap::<_, Vec<String>>::new();
    for item in entities.item_visuals() {
        if let ItemVisualDefinitionRoute::Sprite { texture } = item.route {
            aliases_by_source
                .entry((texture.source, texture.variant))
                .or_default()
                .push(
                    item.key
                        .identifier
                        .strip_prefix("minecraft:")
                        .unwrap_or(&item.key.identifier)
                        .to_owned(),
                );
        }
    }
    let paths = entities
        .item_visuals()
        .iter()
        .filter_map(|item| {
            let ItemVisualDefinitionRoute::Sprite { texture } = item.route else {
                return None;
            };
            let source = entities.sources().get(texture.source as usize)?;
            let aliases = aliases_by_source
                .get(&(texture.source, texture.variant))
                .cloned()
                .unwrap_or_default();
            Some(Route {
                identifier: Arc::from(item.key.identifier.as_ref()),
                metadata: item.key.metadata,
                path: source.path.to_string(),
                variant: texture.variant,
                aliases,
            })
        })
        .collect();
    let _ = PATHS.set(paths);
}

/// Appends only files actually supplied by a pack; absent overrides keep their base icons.
pub(super) fn append(
    view: &LayeredPackView,
    icons: &mut Vec<SessionIcon>,
    retained_bytes: &mut usize,
) {
    let Some(paths) = PATHS.get() else {
        return;
    };
    let mut present = icons
        .iter()
        .map(|icon| (icon.identifier.clone(), icon.metadata))
        .collect::<HashSet<_>>();
    let catalog = super::catalog::paths(view);
    for Route {
        identifier,
        metadata,
        path,
        variant,
        aliases,
    } in paths
    {
        if icons.len() >= MAX_SESSION_ICONS {
            break;
        }
        if present.contains(&(identifier.clone(), *metadata)) {
            continue;
        }
        let stem = path
            .rsplit_once('.')
            .map_or(path.as_str(), |(stem, _)| stem);
        let override_path = aliases
            .iter()
            .find_map(|key| catalog.get(key)?.get(*variant as usize));
        if let Some(mut texture) = pack_runtime::pack_textures::decode_pack_texture_with_policy(
            view,
            override_path.map_or(stem, String::as_str),
            assets::texture::policy::device_policy(),
            true,
        ) {
            texture.retain_first_strip_frame();
            let mut replacement = icon(identifier.clone(), texture);
            if replacement.rgba8.len()
                > assets::texture::policy::MAX_SUBSCRIBER_BYTES.saturating_sub(*retained_bytes)
            {
                continue;
            }
            *retained_bytes += replacement.rgba8.len();
            replacement.metadata = *metadata;
            icons.push(replacement);
            present.insert((identifier.clone(), *metadata));
        }
    }
}

static SPRITES: OnceLock<HashMap<String, assets::IconSprite>> = OnceLock::new();

/// Retains base sprite pixels by their recorded source filenames for custom item aliases.
fn set_vanilla_item_sprites(entities: &RuntimeEntityAssets, icons: &assets::RuntimeIconCatalog) {
    if entities.source_manifest_sha256() != icons.source_manifest_sha256() {
        return;
    }
    let sprites = icons
        .sources()
        .unwrap_or_default()
        .iter()
        .filter_map(|source| {
            let sprite = icons.sprites().get(source.sprite as usize)?.clone();
            Some((source.path.to_string(), sprite))
        })
        .collect();
    let _ = SPRITES.set(sprites);
}

/// Resolves an absent pack raster against a matching immutable base sprite filename, then the
/// base terrain raster of that filename, as the base game's pack sits beneath every pack stack.
pub(super) fn texture(path: &str) -> Option<DecodedTexture> {
    sprite(path).or_else(|| {
        let (keys, base) = super::super::resource_packs::base_material_keys()?;
        terrain_raster(
            TERRAIN_RASTERS.get_or_init(|| terrain_raster_index(keys)),
            base,
            path,
        )
    })
}

/// Copies a base item sprite whose recorded source filename matches `path`.
fn sprite(path: &str) -> Option<DecodedTexture> {
    let sprites = SPRITES.get()?;
    let sprite = pack_runtime::pack_textures::texture_image_paths(path)
        .into_iter()
        .find_map(|path| sprites.get(&path.to_ascii_lowercase()))?;
    Some(DecodedTexture {
        width: u32::from(sprite.width),
        height: u32::from(sprite.height),
        rgba8: sprite.rgba8.to_vec().into_boxed_slice(),
    })
}

/// Base world materials by their lowercase source image filename, built on first use.
static TERRAIN_RASTERS: OnceLock<HashMap<String, u32>> = OnceLock::new();

/// Indexes materials whose carrier pixels equal their source image; the lowest id wins when
/// several states share one image. Materials with a baked atlas tint differ from the file.
fn terrain_raster_index(keys: &MaterialKeys) -> HashMap<String, u32> {
    let mut index = HashMap::new();
    for (material, source) in keys.material_sources() {
        if source.baked_tint.is_none() {
            index
                .entry(source.raster.to_ascii_lowercase())
                .and_modify(|id: &mut u32| *id = (*id).min(material))
                .or_insert(material);
        }
    }
    index
}

/// Copies the first-mip base terrain layer compiled from `path`'s image.
fn terrain_raster(
    index: &HashMap<String, u32>,
    base: &RuntimeAssets,
    path: &str,
) -> Option<DecodedTexture> {
    let material = pack_runtime::pack_textures::texture_image_paths(path)
        .into_iter()
        .find_map(|path| index.get(&path.to_ascii_lowercase()).copied())?;
    let ([width, height], rgba8) = base.material_texels(material)?;
    Some(DecodedTexture {
        width,
        height,
        rgba8,
    })
}

#[cfg(test)]
mod tests;
