//! Carried vanilla block bindings published through the existing session icon consumers.

use super::{BlockIcons, MAX_SESSION_ICONS};
use crate::runtime::network::block_overlay::CompiledBlockOverlay;
use assets::{ItemVisualDefinitionRoute, RuntimeAssets, RuntimeEntityAssets, VisualKind};
use client_ui::ui_runtime::presentation::SessionIcon;
use std::sync::{Arc, OnceLock};

struct Route {
    identifier: Arc<str>,
    metadata: u32,
    visual: u32,
}
static ROUTES: OnceLock<Vec<Route>> = OnceLock::new();

/// Retains item-to-block identities from the same immutable item carrier as sprite routes.
pub(super) fn install(entities: &RuntimeEntityAssets) {
    let _ = ROUTES.set(
        entities
            .item_visuals()
            .iter()
            .filter_map(|item| {
                let ItemVisualDefinitionRoute::BlockItem { block_visual } = item.route else {
                    return None;
                };
                Some(Route {
                    identifier: Arc::from(item.key.identifier.as_ref()),
                    metadata: item.key.metadata,
                    visual: block_visual.0,
                })
            })
            .collect(),
    );
}

/// Adds changed carried thumbnails and cube sheets while retaining their metadata indices.
pub(in crate::runtime::network) fn append(
    base: &RuntimeAssets,
    compiled: &CompiledBlockOverlay,
    icons: &mut BlockIcons,
) {
    let Some(routes) = ROUTES.get() else {
        return;
    };
    append_routes(base, compiled, icons, routes);
}

/// Prepares the supplied carrier routes without changing the installed process-wide inventory.
fn append_routes(
    base: &RuntimeAssets,
    compiled: &CompiledBlockOverlay,
    icons: &mut BlockIcons,
    routes: &[Route],
) {
    for route in routes {
        if icons.icons.len() >= MAX_SESSION_ICONS && icons.block_sheets.len() >= MAX_SESSION_ICONS {
            break;
        }
        let Ok(index) = compiled
            .vanilla_carried
            .binary_search_by_key(&route.visual, |entry| entry.visual)
        else {
            continue;
        };
        let carried = &compiled.vanilla_carried[index];
        let faces = carried.faces;
        let block = base.resolve(assets::NetworkIdMode::Sequential, route.visual);
        if block.kind() != VisualKind::Cube {
            continue;
        }
        if icons.icons.len() < MAX_SESSION_ICONS
            && let Some(sprite) =
                pack_entity_compiler::overlay_cube_icon(&compiled.overlay, faces, None)
        {
            icons.icons.push(session_icon(route, sprite));
        }
        if icons.block_sheets.len() < MAX_SESSION_ICONS
            && let Some(sheet) =
                render_model::equipment::blocks::overlay_face_sheet(&compiled.overlay, faces, None)
        {
            icons.block_sheets.push(session_icon(route, sheet));
            let flags = faces.iter().fold(0, |flags, &id| {
                flags | compiled.overlay.materials[id as usize].flags
            });
            icons
                .block_material_flags
                .insert(Arc::clone(&route.identifier), flags);
        }
    }
}

/// Shares the route's identifier and metadata across the thumbnail and carried sheet.
fn session_icon(route: &Route, sprite: assets::IconSprite) -> SessionIcon {
    SessionIcon {
        identifier: Arc::clone(&route.identifier),
        metadata: route.metadata,
        width: u32::from(sprite.width),
        height: u32::from(sprite.height),
        rgba8: sprite.rgba8.to_vec().into_boxed_slice(),
    }
}

#[cfg(test)]
mod tests;
