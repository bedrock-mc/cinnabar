use crate::valentine::types::{ActorRuntimeId, ActorUniqueId};
use crate::valentine::{
    AvailableActorIdentifiersPacket, BiomeDefinitionListPacket, CreativeContentPacket,
    ItemRegistryPacket, StartGamePacket,
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct WorldTemplate {
    pub start_game_template: StartGamePacket,
    pub item_registry: Arc<ItemRegistryPacket>,
    pub biome_definitions: Arc<BiomeDefinitionListPacket>,
    pub available_entities: Arc<AvailableActorIdentifiersPacket>,
    pub creative_content: Arc<CreativeContentPacket>,
}

#[derive(Debug)]
pub struct WorldJoinParams {
    pub start_game: StartGamePacket,
    pub item_registry: Arc<ItemRegistryPacket>,
    pub biome_definitions: Arc<BiomeDefinitionListPacket>,
    pub available_entities: Arc<AvailableActorIdentifiersPacket>,
    pub creative_content: Arc<CreativeContentPacket>,
}

impl WorldTemplate {
    pub fn to_join_params(&self, entity_id: i64) -> WorldJoinParams {
        let mut start = self.start_game_template.clone();
        start.entity_id = ActorUniqueId {
            actor_unique_id: entity_id,
        };
        start.runtime_id = ActorRuntimeId {
            actor_runtime_id: entity_id,
        };

        WorldJoinParams {
            start_game: start,
            item_registry: self.item_registry.clone(),
            biome_definitions: self.biome_definitions.clone(),
            available_entities: self.available_entities.clone(),
            creative_content: self.creative_content.clone(),
        }
    }
}

impl Default for WorldTemplate {
    fn default() -> Self {
        Self {
            start_game_template: StartGamePacket::default(),
            item_registry: Arc::new(ItemRegistryPacket::default()),
            biome_definitions: Arc::new(BiomeDefinitionListPacket::default()),
            available_entities: Arc::new(AvailableActorIdentifiersPacket::default()),
            creative_content: Arc::new(CreativeContentPacket::default()),
        }
    }
}
