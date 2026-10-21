//! Both recipe-book renderers dispatch primary and secondary crafting gestures.

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
fn both_buttons_craft_from_engine_and_fallback_recipe_books() {
    for workbench in [false, true] {
        for hit in [
            InventoryCellHit::RecipeBook(0),
            InventoryCellHit::Widget(Widget::BookRecipe(0)),
        ] {
            for secondary in [false, true] {
                let (mut player, mut runtime, output) = session(workbench);
                runtime.perform_pointer_action(
                    &mut player,
                    if secondary {
                        PointerAction::SecondaryClick(hit)
                    } else {
                        PointerAction::Click(hit)
                    },
                );
                let ledger = player.inventory.ledger();
                let cursor = ledger.cursor_stack().expect("recipe click must craft");
                assert_eq!(cursor.network_id, output.network_id);
                assert_eq!(cursor.count, u16::from(output.count));
                assert_eq!(
                    u32::from_ne_bytes(cursor.block_runtime_id.to_ne_bytes()),
                    output.block_runtime_id
                );
                assert!(ledger.displayed_stack(0).is_none());
                assert_eq!(ledger.pending_request_count(), 1);
                assert!(ledger.pending_batch().unwrap().is_some());
            }
        }
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
