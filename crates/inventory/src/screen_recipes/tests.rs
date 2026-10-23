use std::sync::Arc;

use super::*;
use protocol::{InventoryEvent, ItemRegistryEvent, NetworkItemStack, decode_recipe_update};

fn uint(out: &mut Vec<u8>, mut n: u32) {
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}

fn int(out: &mut Vec<u8>, n: i32) {
    uint(out, ((n << 1) ^ (n >> 31)) as u32);
}

fn string(out: &mut Vec<u8>, s: &str) {
    uint(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

fn session() -> InventorySession {
    let registry = ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    let update = decode_recipe_update(&recipe_body()).unwrap();
    let mut session = InventorySession::new(1);
    session.ledger_mut().apply_registry(&registry);
    session.crafting_authority.observe(
        1,
        1,
        &crate::InventoryAuthorityEvent::Inventory(InventoryEvent::Recipes(update)),
    );
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(1))));
    session.crafting_authority.advance();
    session
}

/// A CraftingData body with four crafting-table recipes, each a fresh decoded batch.
fn recipe_body() -> Vec<u8> {
    let output = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:crafting_table")
        .unwrap()
        .network_id;
    let mut body = Vec::new();
    uint(&mut body, 4);
    for (id, ingredient, aux) in [
        (1, "minecraft:crimson_planks", 0),
        (2, "minecraft:oak_planks", 0),
        (3, "minecraft:oak_planks", 1),
        (4, "minecraft:oak_planks", 0),
    ] {
        string(&mut body, "test:table");
        int(&mut body, 1);
        int(&mut body, 1);
        uint(&mut body, 1);
        uint(&mut body, 1);
        string(&mut body, "name");
        string(&mut body, ingredient);
        int(&mut body, 0);
        int(&mut body, 4);
        uint(&mut body, 1);
        int(&mut body, output);
        body.extend_from_slice(&1u16.to_le_bytes());
        uint(&mut body, aux);
        int(&mut body, 0);
        uint(&mut body, 0);
        body.extend_from_slice(&[0; 16]);
        string(&mut body, "crafting_table");
        int(&mut body, if id == 4 { 2 } else { 1 });
        body.extend_from_slice(&[0, 0]);
        uint(&mut body, id);
    }
    for _ in 1..11 {
        uint(&mut body, 0);
    }
    body.push(1);
    body
}

#[test]
fn the_recipe_book_lists_one_usable_recipe_per_output() {
    let mut session = session();
    let registry = protocol::vanilla_item_registry();
    let planks = registry
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:oak_planks")
        .unwrap();
    let held = |count| {
        InventoryEvent::Content(protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity::window(0),
            storage_item: NetworkItemStack::empty(),
            slots: Arc::from([NetworkItemStack {
                network_id: planks.network_id,
                count,
                stack_network_id: 1,
                ..NetworkItemStack::empty()
            }]),
        })
    };
    for count in [4, 1] {
        session.ledger_mut().apply(&held(count));
        for filtering in [false, true] {
            let listed = session.book_recipes(filtering, false, 0, usize::MAX);
            assert_eq!(
                listed
                    .iter()
                    .map(|entry| entry.recipe.network_id())
                    .collect::<Vec<_>>(),
                [2, 3]
            );
            assert_eq!(
                session.book_recipes(filtering, false, 1, 1)[0]
                    .recipe
                    .network_id(),
                3
            );
        }
    }
    session.ledger_mut().apply(&held(0));
    assert!(session.book_recipes(true, false, 0, usize::MAX).is_empty());
    assert_eq!(
        session
            .book_recipes(false, false, 0, usize::MAX)
            .iter()
            .map(|entry| entry.recipe.network_id())
            .collect::<Vec<_>>(),
        [1, 3]
    );
}

#[test]
fn recipe_book_validates_each_occupied_source_once_per_projection() {
    let mut session = session();
    let planks = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:oak_planks")
        .unwrap()
        .network_id;
    session
        .ledger_mut()
        .apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity::window(0),
            storage_item: NetworkItemStack::empty(),
            slots: Arc::from([NetworkItemStack {
                network_id: planks,
                count: 4,
                stack_network_id: 1,
                ..NetworkItemStack::empty()
            }]),
        }));
    crate::inventory_ledger::take_source_validations();
    let listed = session.book_recipes(false, false, 0, usize::MAX);
    assert_eq!(
        listed
            .iter()
            .map(|entry| entry.recipe.network_id())
            .collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(
        crate::inventory_ledger::take_source_validations(),
        1,
        "source bytes must be checked once, independent of recipe and ingredient counts"
    );
}

#[test]
fn unchanged_book_inputs_reuse_one_listing_until_materials_change() {
    let mut session = session();
    let planks = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:oak_planks")
        .unwrap()
        .network_id;
    let held = |count| {
        InventoryEvent::Content(protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity::window(0),
            storage_item: NetworkItemStack::empty(),
            slots: Arc::from([NetworkItemStack {
                network_id: planks,
                count,
                stack_network_id: 1,
                ..NetworkItemStack::empty()
            }]),
        })
    };
    session.ledger_mut().apply(&held(4));
    assert_eq!(session.book_recipes(true, false, 0, usize::MAX).len(), 2);
    crate::inventory_ledger::take_source_validations();
    // A screen reads the book several times a frame: icons, rows, pages and hover.
    for _ in 0..8 {
        assert_eq!(session.book_recipes(true, false, 0, usize::MAX).len(), 2);
        assert_eq!(session.book_recipes(false, true, 1, 1).len(), 1);
    }
    assert_eq!(
        crate::inventory_ledger::take_source_validations(),
        0,
        "unchanged inputs reuse the listing without re-checking sources"
    );
    session.ledger_mut().apply(&held(4));
    assert_eq!(session.book_recipes(true, false, 0, usize::MAX).len(), 2);
    assert_eq!(
        crate::inventory_ledger::take_source_validations(),
        0,
        "an identical restatement keeps the listing"
    );
    session.ledger_mut().apply(&held(1));
    assert_eq!(session.book_recipes(true, false, 0, usize::MAX).len(), 2);
    assert_eq!(session.book_recipes(false, false, 0, usize::MAX).len(), 2);
    assert_eq!(
        crate::inventory_ledger::take_source_validations(),
        1,
        "a material change rebuilds the listing once"
    );
}

#[test]
fn a_digest_only_update_rebuilds_the_book_listing() {
    let mut session = session();
    let planks = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:oak_planks")
        .unwrap()
        .network_id;
    let held = |nbt_digest| {
        InventoryEvent::Content(protocol::InventoryContentEvent {
            container: protocol::ContainerIdentity::window(0),
            storage_item: NetworkItemStack::empty(),
            slots: Arc::from([NetworkItemStack {
                network_id: planks,
                count: 4,
                stack_network_id: 1,
                nbt_digest,
                ..NetworkItemStack::empty()
            }]),
        })
    };
    let valid = NetworkItemStack::empty().nbt_digest;
    session.ledger_mut().apply(&held(valid));
    let first = session.book_recipes(false, false, 0, usize::MAX);
    assert!(first.iter().any(|entry| entry.craftable));
    // The same bytes under a digest that no longer matches them can no longer supply a craft.
    session.ledger_mut().apply(&held([7; 32]));
    let listed = session.book_recipes(false, false, 0, usize::MAX);
    assert!(listed.iter().all(|entry| !entry.craftable));
    for entry in &listed {
        assert_eq!(
            entry.craftable,
            session.ledger().can_auto_craft(&entry.recipe),
            "the book agrees with an immediate craftability check"
        );
    }
}

#[test]
fn grid_only_changes_refresh_recipe_book_availability() {
    for workbench in [false, true] {
        let mut session = session();
        if workbench {
            session
                .ledger_mut()
                .apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
                    container: protocol::ContainerIdentity::window(7),
                    window_type: protocol::WINDOW_TYPE_WORKBENCH,
                    position: [0; 3],
                    runtime_entity_id: -1,
                }));
        }
        let planks = protocol::vanilla_item_registry()
            .iter()
            .find(|item| item.identifier.as_ref() == "minecraft:oak_planks")
            .unwrap()
            .network_id;
        let slot = session.ledger().crafting_grid().slots().next().unwrap();
        for count in [0, 4, 1, 0, 4] {
            session
                .ledger_mut()
                .apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
                    identity: protocol::SlotIdentity {
                        container: protocol::ContainerIdentity::window(
                            protocol::UI_INVENTORY_WINDOW_ID,
                        ),
                        slot: u16::from(slot),
                    },
                    stack: NetworkItemStack {
                        network_id: planks,
                        count,
                        stack_network_id: 1,
                        ..NetworkItemStack::empty()
                    },
                    storage_item: None,
                }));
            let listed = session.book_recipes(false, false, 0, usize::MAX);
            assert_eq!(listed.len(), 2);
            assert!(listed.iter().all(|entry| entry.craftable == (count >= 4)));
            assert_eq!(
                session.book_recipes(true, false, 0, usize::MAX).len(),
                if count == 0 { 0 } else { 2 }
            );
        }
    }
}

#[test]
fn block_identity_changes_refresh_recipe_book_availability() {
    let mut session = session();
    let planks = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:oak_planks")
        .unwrap()
        .network_id;
    for block_runtime_id in [0, 1, 0] {
        session
            .ledger_mut()
            .apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(0),
                storage_item: NetworkItemStack::empty(),
                slots: [0, block_runtime_id]
                    .into_iter()
                    .enumerate()
                    .map(|(slot, block_runtime_id)| NetworkItemStack {
                        network_id: planks,
                        count: 2,
                        stack_network_id: slot as i32 + 1,
                        block_runtime_id,
                        ..NetworkItemStack::empty()
                    })
                    .collect(),
            }));
        let listed = session.book_recipes(false, false, 0, usize::MAX);
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .all(|entry| entry.craftable == (block_runtime_id == 0))
        );
    }
}

#[test]
fn replacing_recipes_while_the_book_is_closed_frees_the_old_batch() {
    let mut session = session();
    assert!(!session.book_recipes(false, false, 0, usize::MAX).is_empty());
    let old = session.screen_catalog().unwrap().recipe(2).unwrap();
    let revision = session.screen_catalog().unwrap().revision();
    // A new CraftingData arrives; nothing reads the book afterwards.
    session.crafting_authority.observe(
        1,
        2,
        &crate::InventoryAuthorityEvent::Inventory(InventoryEvent::Recipes(
            decode_recipe_update(&recipe_body()).unwrap(),
        )),
    );
    session.synchronize_crafting_frontier(1, Some((1, 0, Some(2))));
    session.crafting_authority.advance();
    assert_ne!(session.screen_catalog().unwrap().revision(), revision);
    assert_eq!(
        old.batch_owners(),
        1,
        "only this test still holds the replaced batch, so its budget credit can return"
    );
}
