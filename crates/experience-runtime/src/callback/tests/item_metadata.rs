use std::collections::HashMap;

use super::*;
use crate::protocol::ServerItem;

/// Builds a plain server variant with the given stack limit and offhand eligibility.
pub(super) fn server_item(
    id: &str,
    metadata: u16,
    max_count: u8,
    off_hand: bool,
) -> (String, HashMap<u16, ServerItem>) {
    (
        id.to_owned(),
        [(
            metadata,
            ServerItem {
                id: id.to_owned(),
                metadata,
                max_count,
                off_hand,
                plain: true,
            },
        )]
        .into(),
    )
}

/// Unsupported vanilla metadata stages neither inventory writes nor drops and changes no readback.
#[test]
fn unsupported_vanilla_metadata_is_not_staged() {
    let mut res = Fixture::new().res();
    let before = res.inventory().unwrap().unwrap();
    let stack = NewStack {
        metadata: u16::MAX,
        ..new_stack(STONE, 1, None)
    };
    assert_eq!(
        res.set_slot(0, Some(stack.clone())).unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(
        res.drop_item(UP, stack).unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(res.inventory().unwrap().unwrap(), before);
    assert!(res.ops.is_empty());
}

/// A listed vanilla variant preserves its metadata and stack limit in staged readback.
#[test]
fn registered_vanilla_metadata_is_staged_exactly() {
    let mut res = Fixture::new().res();
    let (id, variants) = server_item("minecraft:potion", 1, 1, false);
    Arc::make_mut(&mut res.own).server.insert(id, variants);
    let stack = NewStack {
        id: "minecraft:potion".to_owned(),
        metadata: 1,
        count: 1,
        data: None,
    };
    assert_eq!(res.set_slot(0, Some(stack.clone())).unwrap(), Ok(()));
    let got = res.inventory().unwrap().unwrap().slots[0].clone().unwrap();
    assert_eq!((got.metadata, got.max_count), (1, 1));
    assert_eq!(
        res.set_slot(
            0,
            Some(NewStack {
                metadata: 2,
                ..stack.clone()
            })
        )
        .unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(
        res.set_slot(
            0,
            Some(NewStack {
                count: 2,
                ..stack.clone()
            })
        )
        .unwrap(),
        Err(WorldError::TooLarge)
    );
    assert_eq!(res.inventory().unwrap().unwrap().slots[0], Some(got));
    assert_eq!(res.drop_item(UP, stack.clone()).unwrap(), Ok(()));
    assert_eq!(
        res.ops,
        vec![
            Op::SetSlot {
                slot: 0,
                stack: Some(stack.clone())
            },
            Op::DropItem { pos: UP, stack }
        ]
    );
}

/// Ineligible offhand items are refused without changing staged slots or emitting operations.
#[test]
fn ineligible_offhand_items_are_not_staged() {
    let mut res = Fixture::new().res();
    let before = res.inventory().unwrap().unwrap();
    assert_eq!(
        res.set_slot((INVENTORY_SLOTS - 1) as u32, Some(new_stack(CELL, 1, None)))
            .unwrap(),
        Err(WorldError::UnsupportedState)
    );
    assert_eq!(res.inventory().unwrap().unwrap(), before);
    assert!(res.ops.is_empty());
}

/// An eligible listed item may enter the offhand, and clearing the offhand remains supported.
#[test]
fn eligible_offhand_items_and_clears_are_staged() {
    let mut res = Fixture::new().res();
    let (id, variants) = server_item("minecraft:arrow", 0, 64, true);
    Arc::make_mut(&mut res.own)
        .server
        .insert(id.clone(), variants);
    let slot = (INVENTORY_SLOTS - 1) as u32;
    let stack = NewStack {
        id,
        metadata: 0,
        count: 5,
        data: None,
    };
    assert_eq!(res.set_slot(slot, Some(stack)).unwrap(), Ok(()));
    let got = res.inventory().unwrap().unwrap().slots[slot as usize]
        .clone()
        .unwrap();
    assert_eq!((got.count, got.max_count), (5, 64));
    assert_eq!(res.set_slot(slot, None).unwrap(), Ok(()));
    assert!(res.inventory().unwrap().unwrap().slots[slot as usize].is_none());
}

/// Staged NBT-bearing vanilla stacks keep the adapter's conservative plain classification.
#[test]
fn vanilla_nbt_stacks_match_inventory_classification() {
    let mut res = Fixture::new().res();
    let (id, mut variants) = server_item("minecraft:chest", 0, 64, false);
    variants.get_mut(&0).unwrap().plain = false;
    Arc::make_mut(&mut res.own)
        .server
        .insert(id.clone(), variants);
    assert_eq!(
        res.set_slot(
            0,
            Some(NewStack {
                id,
                metadata: 0,
                count: 1,
                data: None
            })
        )
        .unwrap(),
        Ok(())
    );
    assert!(
        !res.inventory().unwrap().unwrap().slots[0]
            .as_ref()
            .unwrap()
            .plain
    );
}
