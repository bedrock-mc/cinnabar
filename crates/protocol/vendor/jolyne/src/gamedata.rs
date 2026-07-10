//! Game data captured during client login sequence.
//!
//! This module contains the bounded subset of game definition packets required
//! to complete the login/spawn sequence. Optional definition packets remain in
//! the play queue so callers can decode them under their normal work budgets.

use crate::valentine::{
    AvailableEntityIdentifiersPacket, BiomeDefinitionListPacket, CreativeContentPacket,
    ItemRegistryPacket, StartGamePacket,
};

/// Game data captured during the login sequence.
///
/// This struct contains the mandatory game definition packets decoded during
/// the start game sequence. This data is essential for:
/// - Block runtime ID mappings (`block_properties` in `start_game`)
/// - Item registry definitions
///
/// Optional creative, biome, and entity-definition packets are preserved in
/// FIFO order for the play session instead of being eagerly decoded at login.
#[derive(Debug, Clone)]
pub struct GameData {
    /// StartGame packet containing world settings and block properties.
    pub start_game: StartGamePacket,
    /// Item registry with all vanilla and custom items.
    pub item_registry: ItemRegistryPacket,
    /// Reserved for callers that choose to capture biome definitions later.
    /// Client login leaves this unset and queues the packet for play.
    pub biome_definitions: Option<BiomeDefinitionListPacket>,
    /// Reserved for callers that choose to capture entity identifiers later.
    /// Client login leaves this unset and queues the packet for play.
    pub entity_identifiers: Option<AvailableEntityIdentifiersPacket>,
    /// Reserved for callers that choose to capture creative content later.
    /// Client login leaves this unset and queues the packet for play.
    pub creative_content: Option<CreativeContentPacket>,
}
