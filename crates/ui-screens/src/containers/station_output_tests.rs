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

/// Supplies a frame with a resolved station result and no other item artwork.
fn with_output_frame(check: impl FnOnce(ContainerFrame<'_>)) {
    let names = HashMap::new();
    let (icons36, durability36, durability9) = ([None; 36], [None; 36], [None; 9]);
    let (icons54, durability54) = ([None; 54], [None; 54]);
    let (icons4, durability4) = ([None; 4], [None; 4]);
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
        crafting: &crate::inventory_capture::CraftingFrame::default(),
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
    check(frame);
}

#[test]
fn anvil_and_grindstone_output_cells_show_the_predicted_result() {
    with_output_frame(|frame| {
        for window_type in [
            protocol::WINDOW_TYPE_ANVIL,
            protocol::WINDOW_TYPE_GRINDSTONE,
        ] {
            let station = player(window_type);
            let (stack, _, durability) = station_cell(&station, &frame, Cell::Output);
            assert_eq!(stack.map(|stack| stack.network_id), Some(7));
            assert_eq!(durability, Some(0.5));
        }
        let loom = player(protocol::WINDOW_TYPE_LOOM);
        let (stack, _, _) = station_cell(&loom, &frame, Cell::Output);
        assert!(stack.is_none(), "other screens keep their own output path");
    });
}

#[test]
fn trading_result_cells_ignore_a_stale_server_preview() {
    with_output_frame(|mut frame| {
        for modern in [false, true] {
            let mut trader = player_state::PlayerState::new(1);
            trader
                .inventory
                .publish_inventory_authority(protocol::InventoryAuthority::Server);
            let ledger = trader.inventory.ledger_mut();
            ledger.apply(&protocol::InventoryEvent::Trade(protocol::TradeEvent {
                container: protocol::ContainerIdentity::window(7),
                actor_unique_id: 90,
                title: Arc::from("Trader"),
                modern,
                tier: 0,
                offers: Arc::from([]),
            }));
            ledger.apply(&protocol::InventoryEvent::Slot(
                protocol::InventorySlotEvent {
                    identity: protocol::SlotIdentity {
                        container: protocol::ContainerIdentity::window(
                            protocol::UI_INVENTORY_WINDOW_ID,
                        ),
                        slot: u16::from(protocol::CREATED_OUTPUT_SLOT),
                    },
                    stack: NetworkItemStack {
                        network_id: 8,
                        count: 2,
                        ..NetworkItemStack::empty()
                    },
                    storage_item: None,
                },
            ));
            let (stack, _, durability) = station_cell(&trader, &frame, Cell::Output);
            assert_eq!(
                stack.map(|stack| (stack.network_id, stack.count)),
                Some((7, 1))
            );
            assert_eq!(durability, Some(0.5));
            let output = frame.recipe_output.take();
            assert!(station_cell(&trader, &frame, Cell::Output).0.is_none());
            frame.recipe_output = output;
        }
    });
}
