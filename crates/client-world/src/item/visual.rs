use assets::{
    BlockVisualId, ItemVisualDefinitionRoute, ItemVisualId, ItemVisualKey, ItemVisualRoute,
};

use super::ItemStateStore;

#[cfg(test)]
#[path = "visual_route_tests.rs"]
mod tests;

impl ItemStateStore {
    /// Visual route for an item identifier with no stack context (e.g. the TNT block).
    pub(crate) fn visual_for_identifier(&self, identifier: &str) -> ItemVisualRoute {
        self.resolve_visual(identifier, 0, 0)
    }

    /// Resolves an exact visual, using the base block only when aux cannot select a state.
    /// Sprite items keep their metadata-specific visual rules.
    pub(super) fn resolve_visual(
        &self,
        identifier: &str,
        metadata: u32,
        block_runtime_id: i32,
    ) -> ItemVisualRoute {
        let Some(assets) = self.assets.as_ref() else {
            return ItemVisualRoute::Missing;
        };
        let exact = |metadata| {
            let key = ItemVisualKey {
                identifier: identifier.into(),
                metadata,
            };
            if let Ok(index) = assets
                .item_visuals()
                .binary_search_by(|visual| visual.key.cmp(&key))
            {
                return Some(match assets.item_visuals()[index].route {
                    ItemVisualDefinitionRoute::Sprite { .. } => {
                        ItemVisualRoute::Compiled(ItemVisualId(index as u32))
                    }
                    ItemVisualDefinitionRoute::BlockItem { block_visual } => {
                        ItemVisualRoute::BlockItem(BlockVisualId(block_visual.0))
                    }
                    ItemVisualDefinitionRoute::EmptyHand => ItemVisualRoute::EmptyHand,
                    ItemVisualDefinitionRoute::Missing => ItemVisualRoute::Missing,
                });
            }
            assets
                .item_visual_aliases()
                .binary_search_by(|alias| alias.key.cmp(&key))
                .ok()
                .map(|index| ItemVisualRoute::Compiled(assets.item_visual_aliases()[index].visual))
        };
        exact(metadata)
            .or_else(|| {
                (metadata != 0
                    && block_runtime_id == 0
                    && !assets::facts::block_item_has_aux_states(identifier))
                .then(|| exact(0))
                .flatten()
                .filter(|route| matches!(route, ItemVisualRoute::BlockItem(_)))
            })
            .unwrap_or(ItemVisualRoute::Missing)
    }
}

/// Keeps compiled item visuals authoritative, including sprites for items that place blocks.
/// A retained block identity supplies geometry only when no compiled visual resolves.
pub(super) fn classify_retained_block(
    route: ItemVisualRoute,
    block_runtime_id: i32,
) -> ItemVisualRoute {
    if block_runtime_id == 0
        || matches!(
            route,
            ItemVisualRoute::Compiled(_) | ItemVisualRoute::BlockItem(_)
        )
    {
        return route;
    }
    ItemVisualRoute::RetainedBlock { block_runtime_id }
}
