//! The startup facts StartGame and its item registry establish before play.

use std::sync::Arc;

use bedrock_protocol_1_26_51::packets::{ItemRegistry, StartGame};

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
    /// Hash of StartGame's seed, level ID and name, when the server supplies a level ID.
    /// An absent ID cannot establish which world a later connection visits.
    pub level_identity: Option<u64>,
}

/// StartGame's session settings and world rules.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupSettings {
    /// The level's base game version as sent; [`crate::BaseGameVersion::parse`]
    /// resolves the effective version that version-gated behavior reads.
    pub base_game_version: String,
    /// The block-breaking negotiation, separate from authority over any one block.
    pub server_authoritative_block_breaking: bool,
    /// The retained prediction window, in ticks.
    pub rewind_history_size: i32,
    pub hardcore: bool,
    pub inventory_authority: InventoryAuthority,
    pub hud_rules: HudRules,
    pub death_rules: DeathRules,
    pub crafting_rules: crate::CraftingRules,
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
    /// Components retained by the admitted registry after duplicate entries are coalesced.
    pub components: Box<[(Arc<str>, ItemComponents)]>,
    /// `(identifier, item_texture key)` for each admitted item that names or implies an icon.
    pub icon_keys: Box<[(Arc<str>, Arc<str>)]>,
}

impl GameData {
    /// Reads every startup fact from the decoded StartGame and item registry.
    pub(crate) fn from_wire(start_game: &StartGame, item_registry: &ItemRegistry) -> Self {
        let movement = &start_game.movement_settings;
        Self {
            world: WorldBootstrap::from_start_game(start_game),
            environment: WorldEnvironmentBootstrap::from_start_game(start_game),
            settings: StartupSettings {
                base_game_version: start_game
                    .settings
                    .base_game_version
                    .to_string_lossy()
                    .into_owned(),
                server_authoritative_block_breaking: movement.server_authoritative_block_breaking,
                rewind_history_size: movement.rewind_history_size.0,
                hardcore: start_game.settings.is_hardcore,
                inventory_authority: if start_game.enable_item_stack_net_manager {
                    InventoryAuthority::Server
                } else {
                    InventoryAuthority::Client
                },
                hud_rules: HudRules::from_start_game(start_game),
                death_rules: DeathRules::from_start_game(start_game),
                crafting_rules: crate::CraftingRules::from_start_game(start_game),
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
            level_identity: level_identity(start_game),
        }
    }
}

/// Length-framed so the seed, ID and name cannot run into one another.
fn level_identity(start_game: &StartGame) -> Option<u64> {
    if start_game.level_id.is_empty() {
        return None;
    }
    let mut bytes = start_game.settings.seed.0.to_le_bytes().to_vec();
    for text in [&start_game.level_id, &start_game.level_name] {
        bytes.extend_from_slice(&(text.len() as u64).to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    }
    Some(xxhash_rust::xxh64::xxh64(&bytes, 0))
}

impl Default for GameData {
    fn default() -> Self {
        Self::from_wire(&StartGame::default(), &ItemRegistry::default())
    }
}

impl StartupItems {
    fn from_wire(registry: &ItemRegistry) -> Self {
        let normalized = normalize_item_registry(registry.clone()).map_err(WorldPacketError::from);
        let components = normalized
            .as_ref()
            .map(|registry| {
                registry
                    .entries
                    .iter()
                    .filter_map(|entry| {
                        Some((
                            Arc::clone(&entry.identifier),
                            entry.components.as_deref()?.clone(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let icon_keys = normalized
            .as_ref()
            .map(crate::item::item_icon_keys)
            .unwrap_or_default();
        Self {
            registry: normalized,
            identifiers: registry
                .item_data
                .iter()
                .map(|item| Arc::from(item.item_name.to_string_lossy()))
                .collect(),
            components,
            icon_keys,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ItemPacketError, WorldWireError};
    use bedrock_protocol_1_26_51::types::ItemData;
    use bedrock_protocol_1_26_51::wire;

    fn registry(items: &[(&str, i16)]) -> ItemRegistry {
        ItemRegistry {
            item_data: items
                .iter()
                .map(|(name, id)| ItemData {
                    item_name: (*name).into(),
                    item_id: wire::I16LE(*id),
                    ..Default::default()
                })
                .collect(),
        }
    }

    /// Far terrain separates worlds by this; framing keeps "ab"+"c" apart from "a"+"bc".
    #[test]
    fn level_identity_follows_seed_id_and_name() {
        let identity = |seed, id: &str, name: &str| {
            let mut start_game = StartGame::default();
            start_game.settings.seed = wire::U64LE(seed);
            start_game.level_id = id.into();
            start_game.level_name = name.into();
            GameData::from_wire(&start_game, &ItemRegistry::default()).level_identity
        };
        let base = identity(1, "ab", "c");
        assert_eq!(base, identity(1, "ab", "c"));
        assert_ne!(base, identity(2, "ab", "c"));
        assert_ne!(base, identity(1, "a", "bc"));
        assert_ne!(base, identity(1, "ab", "d"));
    }

    /// Names and seeds alone may be shared by unrelated worlds on the same endpoint.
    #[test]
    fn missing_level_id_does_not_claim_a_stable_world_identity() {
        let mut start_game = StartGame::default();
        start_game.settings.seed = wire::U64LE(42);
        start_game.level_name = "Shared world name".into();
        assert_eq!(
            GameData::from_wire(&start_game, &ItemRegistry::default()).level_identity,
            None
        );
    }

    #[test]
    fn item_stack_net_manager_selects_server_inventory_authority() {
        let mut start_game = StartGame::default();
        let client = GameData::from_wire(&start_game, &ItemRegistry::default());
        assert_eq!(
            client.settings.inventory_authority,
            InventoryAuthority::Client
        );
        start_game.enable_item_stack_net_manager = true;
        let server = GameData::from_wire(&start_game, &ItemRegistry::default());
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

    /// One component item whose icon differs from the item's short name.
    fn component_item(icon: &str) -> ItemData {
        let mut data = vec![10, 0, 10, 10];
        data.extend_from_slice(b"components");
        data.extend_from_slice(&[8, 14]);
        data.extend_from_slice(b"minecraft:icon");
        data.push(u8::try_from(icon.len()).unwrap());
        data.extend_from_slice(icon.as_bytes());
        data.extend_from_slice(&[0, 0]);
        ItemData {
            item_name: "test:sword".into(),
            item_id: wire::I16LE(5),
            is_component_based: true,
            item_component_data: wire::NetworkNbt(bytes::Bytes::from(data)),
            ..Default::default()
        }
    }

    #[test]
    fn duplicate_empty_items_preserve_admitted_icon_and_components_in_either_order() {
        let populated = component_item("custom_blade");
        let empty = ItemData {
            item_component_data: wire::NetworkNbt(bytes::Bytes::from_static(&[10, 0, 0])),
            ..populated.clone()
        };
        for item_data in [
            vec![populated.clone(), empty.clone()],
            vec![empty, populated],
        ] {
            let items = StartupItems::from_wire(&ItemRegistry { item_data });
            let registry = items.registry.as_ref().expect("compatible duplicates");
            assert_eq!(registry.entries.len(), 1);
            assert_eq!(
                items.icon_keys.as_ref(),
                &[(Arc::from("test:sword"), Arc::from("custom_blade"))]
            );
            assert_eq!(items.components.len(), 1);
            assert_eq!(items.components[0].0.as_ref(), "test:sword");
            assert_eq!(items.components[0].1.icon.as_deref(), Some("custom_blade"));
            assert_eq!(
                Some(&items.components[0].1),
                registry.entries[0].components.as_deref()
            );
        }
    }

    #[test]
    fn conflicting_components_do_not_publish_presentation_facts() {
        let first = component_item("custom_blade");
        let second = component_item("other_blade");
        let items = StartupItems::from_wire(&ItemRegistry {
            item_data: vec![first, second],
        });
        assert!(items.registry.is_err());
        assert!(items.icon_keys.is_empty());
        assert!(items.components.is_empty());
        assert_eq!(items.identifiers.len(), 2);
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
