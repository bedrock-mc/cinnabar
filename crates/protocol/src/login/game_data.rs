//! The startup facts StartGame and its item registry establish before play.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{ItemRegistryPacket, StartGamePacket};

use crate::{
    CustomBlocks, DeathRules, GameModeUpdate, HudRules, InventoryAuthority, ItemComponents,
    ItemRegistryEvent, PlayerGameMode, WorldBootstrap, WorldEnvironmentBootstrap, WorldPacketError,
    item::normalize_item_registry,
};

/// What StartGame and its item registry establish for one session, read once at login.
/// `Default` is an otherwise empty StartGame with an empty registry.
#[derive(Clone, Debug)]
pub struct GameData {
    pub world: WorldBootstrap,
    pub environment: WorldEnvironmentBootstrap,
    pub settings: StartupSettings,
    pub items: StartupItems,
    /// StartGame's custom block definitions; malformed ones are skipped.
    pub custom_blocks: CustomBlocks,
    /// Every block definition StartGame carried, including vanilla and skipped ones.
    pub block_definition_count: usize,
}

/// StartGame's session settings and world rules.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupSettings {
    /// The world's lighting compatibility version.
    pub base_game_version: String,
    /// The block-breaking negotiation, separate from authority over any one block.
    pub server_authoritative_block_breaking: bool,
    /// The retained prediction window, in ticks.
    pub rewind_history_size: i32,
    pub hardcore: bool,
    pub inventory_authority: InventoryAuthority,
    pub hud_rules: HudRules,
    pub death_rules: DeathRules,
    pub player_game_mode: PlayerGameMode,
    /// The world default, which a later level-default sentinel resolves against.
    pub world_default_game_mode: GameModeUpdate,
    /// Whether the player follows the level default rather than a personal mode.
    pub player_game_mode_uses_world_default: bool,
}

/// The StartGame item registry and the facts its component data declares.
#[derive(Clone, Debug)]
pub struct StartupItems {
    /// The admitted registry, or why normalization rejected it.
    pub registry: Result<ItemRegistryEvent, WorldPacketError>,
    /// Every registry identifier in registry order, admitted or not.
    pub identifiers: Box<[Arc<str>]>,
    /// Components for every item whose component data parses.
    pub components: Box<[(Arc<str>, ItemComponents)]>,
    /// `(identifier, item_texture key)` for every item that names or implies an icon.
    pub icon_keys: Box<[(Arc<str>, Arc<str>)]>,
}

impl GameData {
    /// Reads every startup fact from the decoded StartGame and item registry.
    pub(crate) fn from_wire(
        start_game: &StartGamePacket,
        item_registry: &ItemRegistryPacket,
    ) -> Self {
        let movement = &start_game.movement_settings;
        Self {
            world: WorldBootstrap::from_start_game(start_game),
            environment: WorldEnvironmentBootstrap::from_start_game(start_game),
            settings: StartupSettings {
                base_game_version: start_game.settings.base_game_version.clone(),
                server_authoritative_block_breaking: movement.server_authoritative_block_breaking,
                rewind_history_size: movement.rewind_history_size,
                hardcore: start_game.settings.is_hardcore,
                inventory_authority: if start_game.enable_item_stack_net_manager {
                    InventoryAuthority::Server
                } else {
                    InventoryAuthority::Client
                },
                hud_rules: HudRules::from_start_game(start_game),
                death_rules: DeathRules::from_start_game(start_game),
                player_game_mode: PlayerGameMode::from_start_game(start_game),
                world_default_game_mode: PlayerGameMode::world_default_update_from_start_game(
                    start_game,
                ),
                player_game_mode_uses_world_default: PlayerGameMode::bootstrap_uses_world_default(
                    start_game,
                ),
            },
            items: StartupItems::from_wire(item_registry),
            custom_blocks: CustomBlocks::from_start_game(start_game),
            block_definition_count: start_game.block_properties.len(),
        }
    }
}

impl Default for GameData {
    fn default() -> Self {
        Self::from_wire(&StartGamePacket::default(), &ItemRegistryPacket::default())
    }
}

impl StartupItems {
    fn from_wire(registry: &ItemRegistryPacket) -> Self {
        Self {
            registry: normalize_item_registry(registry.clone()).map_err(WorldPacketError::from),
            identifiers: registry
                .item_data
                .iter()
                .map(|item| Arc::from(item.item_name.as_str()))
                .collect(),
            components: crate::item::item_components(registry),
            icon_keys: crate::item::item_icon_keys(registry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ItemPacketError, WorldWireError};
    use valentine::bedrock::version::v1_26_51::ItemData;

    fn registry(items: &[(&str, i16)]) -> ItemRegistryPacket {
        ItemRegistryPacket {
            item_data: items
                .iter()
                .map(|(name, id)| ItemData {
                    item_name: (*name).into(),
                    item_id: *id,
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[test]
    fn item_stack_net_manager_selects_server_inventory_authority() {
        let mut start_game = StartGamePacket::default();
        let client = GameData::from_wire(&start_game, &ItemRegistryPacket::default());
        assert_eq!(
            client.settings.inventory_authority,
            InventoryAuthority::Client
        );
        start_game.enable_item_stack_net_manager = true;
        let server = GameData::from_wire(&start_game, &ItemRegistryPacket::default());
        assert_eq!(
            server.settings.inventory_authority,
            InventoryAuthority::Server
        );
    }

    #[test]
    fn registry_is_normalized_from_the_captured_packet() {
        let items = StartupItems::from_wire(&registry(&[("minecraft:apple", 878)]));
        let registry = items.registry.expect("normalized registry");
        assert_eq!(registry.entries.len(), 1);
        assert_eq!(registry.entries[0].identifier.as_ref(), "minecraft:apple");
        assert_eq!(registry.entries[0].network_id, 878);
        assert_eq!(&*items.identifiers, [Arc::<str>::from("minecraft:apple")]);
    }

    #[test]
    fn conflicting_registry_ids_are_a_semantic_rejection() {
        let items =
            StartupItems::from_wire(&registry(&[("minecraft:stick", 5), ("minecraft:stone", 5)]));
        assert!(matches!(items.registry, Err(WorldPacketError::Item(_))));
        assert_eq!(
            items.identifiers.len(),
            2,
            "rejected items keep their identifiers"
        );
    }

    #[test]
    fn malformed_registry_nbt_is_a_typed_wire_failure() {
        let mut packet = registry(&[("minecraft:apple", 878)]);
        packet.item_data[0].item_component_data.0 = bytes::Bytes::from_static(&[0xff]);
        assert!(matches!(
            StartupItems::from_wire(&packet).registry,
            Err(WorldPacketError::Wire(WorldWireError::Item(
                ItemPacketError::InvalidItemNbt
            )))
        ));
    }
}
