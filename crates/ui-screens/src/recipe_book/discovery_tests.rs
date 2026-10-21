use super::*;
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, recipe_unlock_event,
};

/// Installs same-output recipes with independent names and ingredient availability.
fn player(granted_sibling: bool) -> player_state::PlayerState {
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let registry = protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    let id = |name: &str| {
        registry
            .entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == name)
            .unwrap()
            .network_id
    };
    let log = id("minecraft:oak_log");
    let output = id("minecraft:oak_planks");
    player.inventory.ledger_mut().apply_registry(&registry);
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
    let mut recipes = vec![RecipeFixture {
        net_id: 1,
        ..RecipeFixture::shapeless(
            "test:locked",
            vec![IngredientFixture::named("minecraft:oak_log", 0)],
            vec![OutputFixture::new(output, 1)],
        )
    }];
    if granted_sibling {
        recipes.push(RecipeFixture {
            net_id: 2,
            ..RecipeFixture::shapeless(
                "test:granted",
                vec![IngredientFixture::named("minecraft:crimson_stem", 0)],
                vec![OutputFixture::new(output, 1)],
            )
        });
    }
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = NetworkItemStack {
        network_id: log,
        count: 8,
        stack_network_id: 11,
        ..NetworkItemStack::empty()
    };
    for (sequence, event) in [
        (
            1,
            protocol::InventoryEvent::Recipes(CraftingDataFixture::clearing(recipes).update()),
        ),
        (
            2,
            recipe_unlock_event(
                1,
                if granted_sibling {
                    &["test:granted"]
                } else {
                    &[]
                },
            ),
        ),
        (
            3,
            protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        ),
    ] {
        player
            .inventory
            .enqueue_inventory_event(1, sequence, event)
            .unwrap();
    }
    player
        .inventory
        .synchronize_crafting_frontier(1, Some((1, 0, Some(3))));
    player.inventory.drain_pending_inventory();
    player
}

#[test]
fn recipe_material_background_counts_ingredients_without_granting_recipe_selection() {
    for granted_sibling in [false, true] {
        let mut player = player(granted_sibling);
        let entry = player.inventory.book_recipes(false, true, 0, 10).remove(0);
        assert_eq!(recipe_background(&player, &entry), ITEM_BACKGROUND);
        assert_eq!(
            player.inventory.book_recipe_selectable(&entry.recipe),
            granted_sibling
        );
        if !granted_sibling {
            assert!(player.inventory.begin_book_craft(&entry.recipe).is_err());
            assert_eq!(player.inventory.ledger().pending_request_count(), 0);
        }
        player
            .inventory
            .ledger_mut()
            .apply(&protocol::InventoryEvent::Content(
                protocol::InventoryContentEvent {
                    container: protocol::ContainerIdentity::window(0),
                    slots: vec![
                        NetworkItemStack::empty();
                        usize::from(protocol::PLAYER_INVENTORY_SLOTS)
                    ]
                    .into(),
                    storage_item: NetworkItemStack::empty(),
                },
            ));
        let entry = player.inventory.book_recipes(false, true, 0, 10).remove(0);
        assert_eq!(recipe_background(&player, &entry), RECIPE_DISABLED);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Creative);
        assert_eq!(recipe_background(&player, &entry), ITEM_BACKGROUND);
    }
}
