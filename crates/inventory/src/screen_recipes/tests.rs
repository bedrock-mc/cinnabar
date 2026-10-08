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
    let output = registry
        .entries
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
    let update = decode_recipe_update(&body).unwrap();
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
            let listed = session.book_recipes(filtering, 0, usize::MAX);
            assert_eq!(
                listed
                    .iter()
                    .map(RecipeHandle::network_id)
                    .collect::<Vec<_>>(),
                [2, 3]
            );
            assert_eq!(session.book_recipes(filtering, 1, 1)[0].network_id(), 3);
        }
    }
    session.ledger_mut().apply(&held(0));
    assert!(session.book_recipes(true, 0, usize::MAX).is_empty());
    assert_eq!(
        session
            .book_recipes(false, 0, usize::MAX)
            .iter()
            .map(RecipeHandle::network_id)
            .collect::<Vec<_>>(),
        [1, 3]
    );
}
