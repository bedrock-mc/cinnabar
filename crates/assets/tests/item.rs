use std::hash::Hash;

use assets::{
    BlockVisualId, ItemActionPhase, ItemIconRef, ItemStackIdentity, ItemVisualId, ItemVisualRoute,
};

fn assert_hash<T: Hash>() {}

fn nonempty_identity(metadata: u32) -> ItemStackIdentity {
    ItemStackIdentity {
        network_id: 42,
        metadata,
        stack_network_id: -1,
        count: 3,
        nbt_digest: [0x5a; 32],
    }
}

#[test]
fn item_identity_and_visual_contracts_are_copyable_values() {
    assert_hash::<ItemStackIdentity>();
    assert_hash::<ItemVisualId>();
    assert_hash::<BlockVisualId>();
    assert_hash::<ItemIconRef>();

    let identity = nonempty_identity(7);
    let identity_copy = identity;
    assert_eq!(identity, identity_copy);

    let compiled = ItemVisualRoute::Compiled(ItemVisualId(9));
    let block = ItemVisualRoute::BlockItem(BlockVisualId(11));
    assert_eq!(compiled, ItemVisualRoute::Compiled(ItemVisualId(9)));
    assert_eq!(block, ItemVisualRoute::BlockItem(BlockVisualId(11)));
    assert_eq!(ItemVisualRoute::EmptyHand, ItemVisualRoute::EmptyHand);
    assert_eq!(ItemVisualRoute::Missing, ItemVisualRoute::Missing);

    let icon = ItemIconRef {
        asset_identity: [0xa5; 32],
        texture_page: 17,
        uv: [1, 2, 3, 4],
    };
    assert_eq!(icon, icon);
}

#[test]
fn empty_identity_has_one_exact_canonical_value() {
    let empty = ItemStackIdentity::empty();
    assert_eq!(
        empty,
        ItemStackIdentity {
            network_id: 0,
            metadata: 0,
            stack_network_id: -1,
            count: 0,
            nbt_digest: [0; 32],
        }
    );
    assert!(empty.is_empty());
}

#[test]
fn zero_count_validation_canonicalizes_the_complete_identity() {
    let noncanonical = ItemStackIdentity {
        network_id: -99,
        metadata: u32::MAX,
        stack_network_id: 812,
        count: 0,
        nbt_digest: [0xff; 32],
    };

    assert_eq!(noncanonical.validate().unwrap(), ItemStackIdentity::empty());
}

#[test]
fn validation_rejects_negative_network_ids_only_for_nonempty_stacks() {
    let invalid = ItemStackIdentity {
        network_id: -1,
        ..nonempty_identity(0)
    };
    assert!(invalid.validate().is_err());

    let valid = nonempty_identity(0);
    assert_eq!(valid.validate().unwrap(), valid);
    assert!(!valid.is_empty());
}

#[test]
fn metadata_remains_lossless_across_the_full_u32_range() {
    for metadata in [u32::MIN, 1, i32::MAX as u32 + 1, u32::MAX] {
        let identity = nonempty_identity(metadata);
        assert_eq!(identity.validate().unwrap().metadata, metadata);
    }
}

#[test]
fn absent_stack_network_identity_uses_the_negative_one_sentinel() {
    let identity = nonempty_identity(0);
    let stack_network_id: i32 = identity.stack_network_id;
    assert_eq!(stack_network_id, -1);
    assert_eq!(identity.validate().unwrap(), identity);
}

#[test]
fn action_phase_contract_carries_exact_tick_progress() {
    assert_eq!(ItemActionPhase::Idle, ItemActionPhase::Idle);
    assert_eq!(
        ItemActionPhase::Windup { elapsed_ticks: 2 },
        ItemActionPhase::Windup { elapsed_ticks: 2 }
    );
    assert_eq!(
        ItemActionPhase::Active { elapsed_ticks: 3 },
        ItemActionPhase::Active { elapsed_ticks: 3 }
    );
    assert_eq!(
        ItemActionPhase::Recover { elapsed_ticks: 4 },
        ItemActionPhase::Recover { elapsed_ticks: 4 }
    );
    assert_eq!(
        ItemActionPhase::UseHeld {
            elapsed_ticks: 5,
            duration_ticks: 20,
        },
        ItemActionPhase::UseHeld {
            elapsed_ticks: 5,
            duration_ticks: 20,
        }
    );
    assert_eq!(ItemActionPhase::Cancelled, ItemActionPhase::Cancelled);
}
