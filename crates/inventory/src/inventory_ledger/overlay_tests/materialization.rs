use sha2::{Digest, Sha256};

use super::*;

/// Builds a valid descriptor of the requested size using an unrelated NBT byte array.
fn padded_stack(extra_bytes: usize) -> NetworkItemStack {
    let mut extra = vec![0xff, 0xff, 1, 10, 0, 0, 7];
    let name = b"padding";
    extra.extend_from_slice(&(name.len() as u16).to_le_bytes());
    extra.extend_from_slice(name);
    let padding_bytes = extra_bytes - extra.len() - 4 - 1 - 8;
    extra.extend_from_slice(&(padding_bytes as i32).to_le_bytes());
    extra.resize(extra.len() + padding_bytes, 0);
    extra.extend_from_slice(&[0; 9]);
    let mut item = stack(6, 10, 4);
    item.nbt_digest = Sha256::digest(&extra).into();
    item.extra_data = extra.into();
    protocol::VerifiedNetworkItemStack::try_new(item.clone(), item.nbt_digest).unwrap();
    item
}

/// Rejects all fields of an oversized correction while keeping the original item usable.
#[test]
fn oversized_response_item_data_preserves_usable_backing() {
    for (name, filtered, damage) in [
        ("", "", 3),
        ("New", "", 0),
        ("New", "Filtered", 0),
        ("New", "Filtered", 3),
    ] {
        let original = padded_stack(protocol::MAX_ITEM_EXTRA_BYTES);
        let mut ledger = open_ledger(&[(0, original.clone())]);
        let request = ledger
            .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
            .unwrap();
        send_all(&mut ledger);
        let revision = ledger.authoritative_slot_revision(0).unwrap();
        let mut reply = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 2, 12);
        let slot = &mut Arc::make_mut(&mut reply.slots)[0];
        slot.custom_name = Arc::from(name);
        slot.filtered_custom_name = Arc::from(filtered);
        slot.durability_correction = damage;
        respond(
            &mut ledger,
            request,
            StackResponseStatus::Accepted,
            vec![reply],
        );

        assert_eq!(
            ledger.authoritative_slot_revision(0),
            Some(revision),
            "skipped corrections must preserve slot authority: {name:?}, {filtered:?}, {damage}"
        );
        let retained = ledger.displayed_stack(0).unwrap();
        assert!(
            protocol::VerifiedNetworkItemStack::try_new(retained.clone(), retained.nbt_digest)
                .is_ok(),
            "correction must not retain an unusable descriptor: {name:?}, {filtered:?}, {damage}"
        );
        assert!(retained == &original, "oversized correction must be atomic");
        assert!(ledger.presented_slot_overlay(0).is_none());
        assert_eq!(ledger.skipped_unknown_containers(), 1);
        assert_eq!(ledger.pending_request_count(), 0);
        assert!(!ledger.resync_required());
        ledger.begin_world_drop(0, Some(1)).unwrap();
        assert!(ledger.pending_batch().unwrap().is_some());
    }
}

/// Corrections may fill the descriptor exactly and still serialize in a later normal drop.
#[test]
fn response_item_data_at_descriptor_limit_remains_usable() {
    let edit = protocol::ItemNbtEdit {
        damage: Some(3),
        custom_name: Some(Some("New".into())),
        filtered_custom_name: Some(Some("Filtered".into())),
        ..Default::default()
    };
    let probe = padded_stack(64);
    let growth = protocol::edit_item_extra(&probe.extra_data, &edit)
        .unwrap()
        .len()
        - probe.extra_data.len();
    let original = padded_stack(protocol::MAX_ITEM_EXTRA_BYTES - growth);
    let mut ledger = open_ledger(&[(0, original)]);
    let request = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    let revision = ledger.authoritative_slot_revision(0).unwrap();
    let mut reply = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 2, 12);
    let slot = &mut Arc::make_mut(&mut reply.slots)[0];
    slot.custom_name = Arc::from("New");
    slot.filtered_custom_name = Arc::from("Filtered");
    slot.durability_correction = 3;
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![reply],
    );

    assert_eq!(ledger.authoritative_slot_revision(0), Some(revision + 1));
    let retained = ledger.displayed_stack(0).unwrap();
    assert_eq!(retained.extra_data.len(), protocol::MAX_ITEM_EXTRA_BYTES);
    assert_eq!((retained.count, retained.stack_network_id), (2, 12));
    assert_eq!(
        protocol::item_custom_name(&retained.extra_data).as_deref(),
        Some("New")
    );
    assert_eq!(
        protocol::item_filtered_custom_name(&retained.extra_data).as_deref(),
        Some("Filtered")
    );
    assert_eq!(
        protocol::item_extra_damage_value(&retained.extra_data),
        Some(3)
    );
    protocol::VerifiedNetworkItemStack::try_new(retained.clone(), retained.nbt_digest).unwrap();
    assert_eq!(ledger.skipped_unknown_containers(), 0);
    ledger.begin_world_drop(0, Some(1)).unwrap();
    assert!(ledger.pending_batch().unwrap().is_some());
}
