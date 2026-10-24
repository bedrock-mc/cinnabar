//! The pack's id tables for Cinnabar's Bedrock content: `mc_Entity` per block state
//! (`block.properties`), and `entityId` / item ids per Bedrock identifier (`entity.properties`,
//! `item.properties`), each through the pinned Java mapping (Geyser's block, entity and item
//! mappings). Identifiers the mapping lacks are looked up by their own name.

use std::sync::Arc;

use assets::facts::JavaBlockMapping;
use render_scene::shaderpack::ShaderPackIdMaps;
use shaderpack::NamedIdMap;

/// The pack's entity and item ids, keyed by Java id.
#[derive(Debug, Default)]
pub(crate) struct ActorIdMaps {
    entities: NamedIdMap,
    items: NamedIdMap,
    mapping: Option<&'static JavaBlockMapping>,
}

impl ActorIdMaps {
    pub(crate) fn new(
        entities: NamedIdMap,
        items: NamedIdMap,
        mapping: Option<&'static JavaBlockMapping>,
    ) -> Self {
        Self {
            entities,
            items,
            mapping,
        }
    }

    /// The `entityId` of a Bedrock entity identifier (`minecraft:ender_crystal`).
    pub(crate) fn entity_id(&self, bedrock_identifier: &str) -> Option<i32> {
        match self.mapping {
            Some(mapping) => self.entities.bedrock_entity_id(mapping, bedrock_identifier),
            None => self.entities.id(bedrock_identifier),
        }
    }

    /// The item id of a Bedrock item identifier.
    pub(crate) fn item_id(&self, bedrock_identifier: &str) -> Option<i32> {
        match self.mapping {
            Some(mapping) => self.items.bedrock_item_id(mapping, bedrock_identifier),
            None => self.items.id(bedrock_identifier),
        }
    }

    /// The main-world lookups the app resolves actors and held items with.
    pub(crate) fn lookups(self: &Arc<Self>, revision: u64) -> ShaderPackIdMaps {
        let entities = Arc::clone(self);
        let items = Arc::clone(self);
        ShaderPackIdMaps::new(
            Arc::new(move |name: &str| entities.entity_id(name)),
            Arc::new(move |name: &str| items.item_id(name)),
            revision,
        )
    }
}

/// The pack's `mc_Entity` per Bedrock block state (by sequential id; `None` without the
/// pinned mapping) and its entity and item ids, from the pack's parsed `maps`.
pub(crate) fn id_tables(
    maps: shaderpack::IdMaps,
    blocks: Option<Vec<i32>>,
    name: &str,
) -> (Option<Arc<Vec<i32>>>, Arc<ActorIdMaps>) {
    for diagnostic in &maps.diagnostics {
        bevy::log::debug!(
            target: "cinnabar::shaderpack",
            pack = name, line = diagnostic.line,
            "id maps: {}", diagnostic.message
        );
    }
    let mapping = match JavaBlockMapping::pinned() {
        Ok(mapping) => Some(mapping),
        Err(error) => {
            bevy::log::warn!(
                target: "cinnabar::shaderpack",
                pack = name,
                "no Java mapping: every mc_Entity reads -1 and entity and item ids match by \
                 Bedrock name: {error}"
            );
            None
        }
    };
    let blocks = blocks.map(Arc::new);
    let actors = Arc::new(ActorIdMaps::new(maps.entities, maps.items, mapping));
    (blocks, actors)
}
