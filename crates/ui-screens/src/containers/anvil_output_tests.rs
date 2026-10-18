use std::{collections::HashMap, sync::Arc};

use protocol::NetworkItemStack;

use super::{Cell, station_cell};
use crate::container_source::{ContainerFrame, StationOutput};

/// A player with an open window of `window_type` and no server-stated output.
fn player(window_type: i8) -> player_state::PlayerState {
    let mut player = player_state::PlayerState::new(1);
    player
        .inventory
        .publish_inventory_authority(protocol::InventoryAuthority::Server);
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Open(
            protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(5),
                window_type,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            },
        ));
    player
}

#[test]
fn the_anvil_output_cell_shows_the_predicted_result() {
    let names = HashMap::new();
    let (icons36, durability36, durability9) = ([None; 36], [None; 36], [None; 9]);
    let (icons54, durability54) = ([None; 54], [None; 54]);
    let (icons9, icons4, durability4) = ([None; 9], [None; 4], [None; 4]);
    let entries: Arc<[Option<ui::IconRef>]> = Arc::from([]);
    let predicted = StationOutput {
        icon: None,
        durability: Some(0.5),
        stack: NetworkItemStack {
            network_id: 7,
            count: 1,
            ..NetworkItemStack::empty()
        },
    };
    let frame = ContainerFrame {
        item_names: &names,
        inventory_icons: &icons36,
        player_durability: &durability36,
        hotbar_durability: &durability9,
        storage_icons: &icons54,
        storage_durability: &durability54,
        ui_icons: &[],
        ui_durability: &[],
        crafting_icons: &icons9,
        crafting_output: None,
        armor_icons: &icons4,
        armor_durability: &durability4,
        offhand_icon: None,
        offhand_durability: None,
        cursor_icon: None,
        cursor_durability: None,
        recipe_icons: &[],
        recipe_output: Some(&predicted),
        book_entries: &[],
        furnace_entries: &entries,
    };
    let anvil = player(protocol::WINDOW_TYPE_ANVIL);
    let (stack, _, durability) = station_cell(&anvil, &frame, Cell::Output);
    assert_eq!(stack.map(|stack| stack.network_id), Some(7));
    assert_eq!(durability, Some(0.5));
    let loom = player(protocol::WINDOW_TYPE_LOOM);
    let (stack, _, _) = station_cell(&loom, &frame, Cell::Output);
    assert!(stack.is_none(), "other screens keep their own output path");
}
