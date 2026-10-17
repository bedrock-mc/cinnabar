//! Ordinary recipe matching keeps input item data separate from ingredient identity.

use super::*;
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};
use sha2::{Digest, Sha256};

/// Gives one original ingredient a valid custom name without changing its item identity.
fn named_input() -> NetworkItemStack {
    let mut input = stack(LOG, 101, 2);
    input.extra_data = protocol::edit_item_extra(
        &[0; 10],
        &protocol::ItemNbtEdit {
            custom_name: Some(Some("Craft ingredient".to_owned())),
            ..Default::default()
        },
    )
    .unwrap();
    input.nbt_digest = Sha256::digest(&input.extra_data).into();
    input
}

/// Admits an ordinary named or tagged recipe through the production catalog.
fn ordinary_recipe(shaped: bool, tagged: bool) -> (RecipeCatalog, protocol::RecipeHandle) {
    let ingredient = if tagged {
        IngredientFixture::tagged("test:wood")
    } else {
        IngredientFixture::named("minecraft:oak_log", 0)
    };
    let output = vec![OutputFixture::new(PLANKS, 4)];
    let mut recipe = if shaped {
        RecipeFixture::shaped("test:item_data", 1, 1, vec![ingredient], output)
    } else {
        RecipeFixture::shapeless("test:item_data", vec![ingredient], output)
    };
    recipe.net_id = 55;
    let mut catalog = RecipeCatalog::default();
    catalog.begin_session(1);
    assert!(catalog.apply(1, 1, &CraftingDataFixture::clearing(vec![recipe]).update()));
    let handle = catalog.recipe(55).unwrap();
    (catalog, handle)
}

/// Supplies the same negotiated tag to both manual and automatic ingredient matching.
fn named_ledger() -> PlayerInventoryLedger {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    let mut entries = registry().entries.to_vec();
    entries
        .iter_mut()
        .find(|entry| entry.network_id == LOG)
        .unwrap()
        .item_tags = Arc::from([Arc::from("test:wood")]);
    ledger.apply_registry(&ItemRegistryEvent {
        entries: entries.into(),
    });
    ledger
}

#[test]
fn item_data_ordinary_manual_recipes_consume_named_inputs_without_copying_the_name() {
    for (shaped, tagged) in [(true, false), (false, false), (false, true)] {
        let mut ledger = named_ledger();
        let input = named_input();
        ledger.apply(&craft_slot(28, input.clone()));
        let (catalog, recipe) = ordinary_recipe(shaped, tagged);
        assert_eq!(unique(&ledger, &catalog).network_id(), recipe.network_id());
        let request = ledger.begin_craft(&recipe, 1).unwrap();
        let remaining = &ledger.view().get(Cell::Craft(28)).unwrap().stack;
        assert_eq!((remaining.count, remaining.stack_network_id), (1, request));
        assert_eq!(remaining.extra_data, input.extra_data);
        let output = ledger.cursor_stack().unwrap();
        assert_eq!((output.network_id, output.count), (PLANKS, 4));
        assert!(protocol::item_custom_name(&output.extra_data).is_none());
        assert!(
            ledger
                .newest_request()
                .unwrap()
                .actions
                .iter()
                .any(|action| matches!(action,
            StackRequestAction::Consume { amount: 1, source } if source.stack_network_id == 101))
        );
    }
}

#[test]
fn item_data_recipe_book_consumes_named_inputs_with_their_current_stack_identity() {
    for tagged in [false, true] {
        let mut ledger = named_ledger();
        let input = named_input();
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 9,
            },
            stack: input.clone(),
            storage_item: None,
        }));
        let (_, recipe) = ordinary_recipe(false, tagged);
        assert!(ledger.can_auto_craft(&recipe));
        let request = ledger.begin_auto_craft(&recipe).unwrap();
        let remaining = ledger.displayed_stack(9).unwrap();
        assert_eq!((remaining.count, remaining.stack_network_id), (1, request));
        assert_eq!(remaining.extra_data, input.extra_data);
        assert_eq!(ledger.cursor_stack().unwrap().count, 4);
        assert!(ledger.newest_request().unwrap().actions.iter().any(|action| matches!(action,
            StackRequestAction::Consume { amount: 1, source } if source.slot == 9 && source.stack_network_id == 101)));
    }
}

#[test]
fn item_data_corrupted_retained_bytes_cannot_supply_an_ordinary_recipe() {
    let mut ledger = named_ledger();
    let mut input = named_input();
    input.nbt_digest = [0; 32];
    ledger.apply(&craft_slot(28, input.clone()));
    let (_, recipe) = ordinary_recipe(false, false);
    assert!(ledger.crafting_grid_cells().is_none());
    assert!(ledger.begin_craft(&recipe, 1).is_err());
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 9,
        },
        stack: input,
        storage_item: None,
    }));
    assert!(!ledger.can_auto_craft(&recipe));
    assert!(ledger.begin_auto_craft(&recipe).is_err());
    assert_eq!(ledger.pending_request_count(), 0);
}
