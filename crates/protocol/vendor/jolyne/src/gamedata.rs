//! Game data captured during client login sequence.
//!
//! This module contains the [`GameData`] struct which holds all the game definition
//! packets received from the server during the login/spawn sequence.

use crate::valentine::{
    AvailableEntityIdentifiersPacket, BiomeDefinitionListPacket, CreativeContentPacket,
    ItemRegistryPacket, StartGamePacket,
};

/// Game data captured during the login sequence.
///
/// This struct contains all the game definition packets sent by the server
/// during the start game sequence. This data is essential for:
/// - Block runtime ID mappings (`block_properties` in `start_game`)
/// - Item registry definitions
/// - Creative inventory content
/// - Biome definitions
/// - Entity identifiers
#[derive(Debug, Clone)]
pub struct GameData {
    /// StartGame packet containing world settings and block properties.
    pub start_game: StartGamePacket,
    /// Item registry with all vanilla and custom items.
    pub item_registry: ItemRegistryPacket,
    /// Biome definitions (if received).
    pub biome_definitions: Option<BiomeDefinitionListPacket>,
    /// Available entity identifiers (if received).
    pub entity_identifiers: Option<AvailableEntityIdentifiersPacket>,
    /// Creative content for the creative inventory (if received).
    pub creative_content: Option<CreativeContentPacket>,
}
