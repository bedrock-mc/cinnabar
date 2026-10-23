//! Both recipe-book renderers dispatch primary placement and secondary crafting gestures.

use super::*;
use crate::ui_runtime::{inventory_actions::recipe_book_entries, inventory_drag::PointerAction};
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};
use protocol::{ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent};
use ui_screens::container_actions::{InventoryCellHit, Widget};

/// Admits a wire recipe, stocked inventory and acknowledged crafting screen.
fn session(workbench: bool) -> (player_state::PlayerState, UiRuntime, protocol::RecipeOutput) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let registry = protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    let id = |name| {
        registry
            .entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == name)
            .unwrap()
            .network_id
    };
    let log = id("minecraft:oak_log");
    let planks = id("minecraft:oak_planks");
    runtime.publish_crafting_bootstrap(
        &mut player,
        Some(&registry),
        protocol::InventoryAuthority::Server,
    );
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&registry);
    let mut slots =
        vec![protocol::NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = protocol::NetworkItemStack {
        network_id: log,
        stack_network_id: 101,
        count: 1,
        ..protocol::NetworkItemStack::empty()
    };
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: slots.into(),
            storage_item: protocol::NetworkItemStack::empty(),
        }));
    let recipe = RecipeFixture {
        net_id: 77,
        ..RecipeFixture::shaped(
            "test:quick_craft",
            1,
            1,
            vec![IngredientFixture::named("minecraft:oak_log", 0)],
            vec![OutputFixture {
                block_runtime_id: i32::MIN + 1,
                ..OutputFixture::new(planks, 4)
            }],
        )
    };
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            InventoryEvent::Recipes(CraftingDataFixture::clearing(vec![recipe]).update()),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(1))));
    runtime.drain_pending_inventory(&mut player);
    let window_type = if workbench {
        protocol::WINDOW_TYPE_WORKBENCH
    } else {
        runtime.toggle_inventory(&mut player, None);
        assert!(
            runtime
                .inventory_ledger_mut(&mut player)
                .mark_transport_enqueued(0)
        );
        inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE
    };
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            2,
            InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(2))));
    runtime.drain_pending_inventory(&mut player);
    let output = runtime.book_recipes(&player, 0, 1)[0].recipe.output();
    (player, runtime, output)
}

#[test]
fn a_secondary_click_crafts_from_engine_and_fallback_recipe_books() {
    for workbench in [false, true] {
        for hit in [
            InventoryCellHit::RecipeBook(0),
            InventoryCellHit::Widget(Widget::BookRecipe(0)),
        ] {
            let (mut player, mut runtime, output) = session(workbench);
            runtime.perform_pointer_action(&mut player, PointerAction::SecondaryClick(hit));
            let ledger = player.inventory.ledger();
            // The slot the craft empties never takes its output.
            let crafted = ledger.displayed_stack(1).expect("recipe click must craft");
            assert_eq!(crafted.network_id, output.network_id);
            assert_eq!(crafted.count, u16::from(output.count));
            assert_eq!(
                u32::from_ne_bytes(crafted.block_runtime_id.to_ne_bytes()),
                output.block_runtime_id
            );
            assert!(ledger.cursor_stack().is_none());
            assert!(ledger.displayed_stack(0).is_none());
            assert_eq!(ledger.pending_request_count(), 1);
            assert!(ledger.pending_batch().unwrap().is_some());
        }
    }
}

/// A primary click moves the ingredients into the grid rather than crafting.
#[test]
fn a_primary_click_lays_the_recipe_into_the_grid() {
    for workbench in [false, true] {
        for hit in [
            InventoryCellHit::RecipeBook(0),
            InventoryCellHit::Widget(Widget::BookRecipe(0)),
        ] {
            let (mut player, mut runtime, _) = session(workbench);
            let log = player
                .inventory
                .ledger()
                .displayed_stack(0)
                .unwrap()
                .network_id;
            runtime.perform_pointer_action(&mut player, PointerAction::Click(hit));
            let ledger = player.inventory.ledger();
            let first = ledger.crafting_grid().slots().next().unwrap();
            let placed = ledger
                .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(first))
                .expect("the log moves into the grid");
            assert_eq!((placed.network_id, placed.count), (log, 1));
            assert!(ledger.displayed_stack(0).is_none());
            assert!(ledger.displayed_stack(1).is_none());
            assert_eq!(ledger.pending_request_count(), 1);
        }
    }
}

/// After a primary click moves the only log into the grid, a secondary click crafts from it.
#[test]
fn a_secondary_click_crafts_from_ingredients_a_primary_click_placed() {
    for workbench in [false, true] {
        let (mut player, mut runtime, output) = session(workbench);
        let hit = InventoryCellHit::RecipeBook(0);
        runtime.perform_pointer_action(&mut player, PointerAction::Click(hit));
        runtime.perform_pointer_action(&mut player, PointerAction::SecondaryClick(hit));
        let ledger = player.inventory.ledger();
        assert_eq!(ledger.pending_request_count(), 2);
        let first = ledger.crafting_grid().slots().next().unwrap();
        assert!(
            ledger
                .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(first))
                .is_none()
        );
        let crafted = ledger
            .displayed_stack(0)
            .expect("the craft lands in the hotbar");
        assert_eq!(
            (crafted.network_id, crafted.count),
            (output.network_id, u16::from(output.count))
        );
    }
}

#[test]
fn a_held_stack_stops_a_secondary_recipe_click_and_shift_crafts_a_stack() {
    for hit in [
        InventoryCellHit::RecipeBook(0),
        InventoryCellHit::Widget(Widget::BookRecipe(0)),
    ] {
        let (mut player, mut runtime, output) = session(false);
        let log = player
            .inventory
            .ledger()
            .displayed_stack(0)
            .unwrap()
            .clone();
        runtime
            .inventory_ledger_mut(&mut player)
            .apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                    slot: 0,
                },
                stack: protocol::NetworkItemStack {
                    stack_network_id: 300,
                    ..log
                },
                storage_item: None,
            }));
        assert!(player.inventory.ledger().cursor_stack().is_some());
        runtime.perform_pointer_action(&mut player, PointerAction::SecondaryClick(hit));
        assert_eq!(
            player.inventory.ledger().pending_request_count(),
            0,
            "{hit:?}"
        );
        runtime.perform_pointer_action(&mut player, PointerAction::QuickMove(hit));
        let ledger = player.inventory.ledger();
        assert_eq!(ledger.pending_request_count(), 1);
        let crafted = ledger.displayed_stack(1).expect("shift-click must craft");
        assert_eq!(
            (crafted.network_id, crafted.count),
            (output.network_id, u16::from(output.count))
        );
    }
}

#[test]
fn recipe_book_display_retains_the_output_block_identity() {
    let (player, runtime, output) = session(false);
    let entries = recipe_book_entries(&player, &runtime);
    let stack = entries[0].stack();
    assert_eq!(
        u32::from_ne_bytes(stack.block_runtime_id.to_ne_bytes()),
        output.block_runtime_id
    );
}
