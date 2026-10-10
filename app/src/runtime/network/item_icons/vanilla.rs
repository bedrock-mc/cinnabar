//! Optional raster replacements follow source paths recorded by the pinned item carrier.
use super::{MAX_SESSION_ICONS, first_frame, icon};
use assets::{ItemVisualDefinitionRoute, RuntimeEntityAssets};
use client_ui::ui_runtime::presentation::SessionIcon;
use resource_pack::LayeredPackView;
use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
};

struct Route {
    identifier: Arc<str>,
    metadata: u32,
    path: String,
    variant: u32,
    aliases: Vec<String>,
}
static PATHS: OnceLock<Vec<Route>> = OnceLock::new();

/// Whether the carrier's item routes are installed; once installed they never change.
pub(crate) fn vanilla_item_paths_installed() -> bool {
    PATHS.get().is_some()
}

/// Retains source routes once, so menus can apply packs before any item registry arrives.
pub(crate) fn set_vanilla_item_paths(entities: &RuntimeEntityAssets) {
    let mut aliases_by_source = std::collections::HashMap::<_, Vec<String>>::new();
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
pub(super) fn append(view: &LayeredPackView, icons: &mut Vec<SessionIcon>) {
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
        if let Some(texture) = super::super::resource_packs::decode_pack_texture(
            view,
            override_path.map_or(stem, String::as_str),
        ) {
            let mut replacement = icon(identifier.clone(), first_frame(texture));
            replacement.metadata = *metadata;
            icons.push(replacement);
            present.insert((identifier.clone(), *metadata));
        }
    }
}
