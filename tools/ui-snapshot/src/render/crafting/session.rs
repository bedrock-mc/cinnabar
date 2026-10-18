//! Original negotiated inventory and wire-decoded recipe for the overlapping-input capture.

use std::sync::Arc;

use inventory::inventory_ledger::{PERSONAL_INVENTORY_WINDOW_TYPE, PLAYER_INVENTORY_SLOT_COUNT};
use player_state::PlayerState;
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, NetworkItemStack,
    PlayerGameMode,
    test_support::recipes::{CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture},
};

pub const LOG: i32 = 6;
pub const PLANKS: i32 = 7;
pub const FURNACE: i32 = 9;
const LOG_NAME: &str = "minecraft:oak_log";
const TAG: &str = "test:wood";
const OTHER_TAG: &str = "test:other";

pub struct Item {
    pub id: i32,
    identifier: &'static str,
    pub label: &'static str,
    pub color: [u8; 3],
    tags: &'static [&'static str],
}

/// One source for registry facts, item names and the original palette's item order.
pub const ITEMS: [Item; 4] = [
    Item {
        id: LOG,
        identifier: LOG_NAME,
        label: "Fixture log",
        color: [139, 91, 52],
        tags: &[TAG],
    },
    Item {
        id: PLANKS,
        identifier: "minecraft:oak_planks",
        label: "Fixture planks",
        color: [211, 171, 105],
        tags: &[TAG, OTHER_TAG],
    },
    Item {
        id: 8,
        identifier: "minecraft:cobblestone",
        label: "Fixture stone",
        color: [101, 101, 101],
        tags: &[OTHER_TAG],
    },
    Item {
        id: FURNACE,
        identifier: "minecraft:furnace",
        label: "Fixture result",
        color: [118, 135, 148],
        tags: &[],
    },
];

/// Admits fixture facts through the owning session, then acknowledges its personal-open request.
pub fn player() -> Result<PlayerState, String> {
    let mut player = PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let registry = ItemRegistryEvent {
        entries: ITEMS
            .iter()
            .map(|item| ItemRegistryEntry {
                identifier: item.identifier.into(),
                network_id: item.id,
                component_based: false,
                version: ItemRegistryVersion::None,
                components: None,
                component_digest: [0; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: true,
                item_tags: item.tags.iter().map(|tag| Arc::from(*tag)).collect(),
            })
            .collect(),
    };
    if !player.inventory.publish_bootstrap_inventory(
        Some(registry),
        InventoryEvent::Authority(InventoryAuthority::Server),
    ) {
        return Err("craft fixture bootstrap was refused".into());
    }
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    for (offset, network_id) in [LOG, PLANKS].into_iter().enumerate() {
        slots[usize::from(protocol::HOTBAR_SLOT_COUNT) + offset] = NetworkItemStack {
            network_id,
            stack_network_id: 101 + offset as i32,
            count: 1,
            ..NetworkItemStack::empty()
        };
    }
    let mut recipe = RecipeFixture::shapeless(
        "test:overlap",
        vec![
            IngredientFixture::tagged(TAG),
            IngredientFixture::named(LOG_NAME, 0),
        ],
        vec![OutputFixture::new(FURNACE, 1)],
    );
    recipe.net_id = 77;
    for (sequence, event) in [
        InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: slots.into(),
            storage_item: NetworkItemStack::empty(),
        }),
        InventoryEvent::Recipes(CraftingDataFixture::clearing(vec![recipe]).update()),
    ]
    .into_iter()
    .enumerate()
    {
        player
            .inventory
            .enqueue_inventory_event(1, sequence as u64 + 1, event)
            .map_err(|error| error.to_string())?;
    }
    player
        .inventory
        .synchronize_crafting_frontier(1, Some((1, 0, Some(2))));
    player.inventory.drain_pending_inventory();
    if player
        .inventory
        .book_recipes(false, false, 0, usize::MAX)
        .len()
        != 1
    {
        return Err("craft fixture recipe was not committed".into());
    }
    let ledger = player.inventory.ledger_mut();
    if !ledger.request_personal_open(42) || !ledger.mark_transport_enqueued(0) {
        return Err("craft fixture personal-open request was refused".into());
    }
    player
        .inventory
        .enqueue_inventory_event(
            1,
            3,
            InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(2),
                window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .map_err(|error| error.to_string())?;
    player
        .inventory
        .synchronize_crafting_frontier(1, Some((1, 0, Some(3))));
    player.inventory.drain_pending_inventory();
    Ok(player)
}
