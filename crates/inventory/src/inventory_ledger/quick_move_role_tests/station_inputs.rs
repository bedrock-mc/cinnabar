//! Stack-aware station admission across the public gesture builders.

use super::*;
use protocol_ids::ContainerName;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Resolves the first UI cell from the shared protocol layout.
fn target(name: ContainerName) -> InventoryTarget {
    let slot = (0..protocol::UI_SLOT_COUNT)
        .filter_map(|slot| u8::try_from(slot).ok())
        .find(|slot| protocol::ui_slot_container_name(*slot) == Some(name.wire_value()))
        .expect("named station input");
    InventoryTarget::Craft(slot)
}

/// Encodes original typed root fields in an otherwise empty item extra blob.
fn extra(fields: &[(u8, &str, &[u8])]) -> Arc<[u8]> {
    let mut bytes = vec![255, 255, 1, 10, 0, 0];
    for (tag, name, payload) in fields {
        bytes.push(*tag);
        bytes.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend(name.as_bytes());
        bytes.extend(*payload);
    }
    bytes.push(0);
    bytes.extend([0; 8]);
    bytes.into()
}

/// Publishes a registered source and original metadata before any prediction begins.
fn source(window: i8, entry: &ItemRegistryEntry, data: Arc<[u8]>) -> PlayerInventoryLedger {
    let mut ledger = station_ledger(entry, window);
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[usize::from(SOURCE_SLOT)] = NetworkItemStack {
        extra_data: data.clone(),
        nbt_digest: Sha256::digest(&data).into(),
        ..ledger.displayed_stack(SOURCE_SLOT).unwrap().clone()
    };
    ledger.apply(&content(ContainerIdentity::window(0), slots));
    ledger
}

/// Tries cursor, counted, swapped, hotbar and drag placements without publishing replies.
#[test]
fn station_inputs_refuse_foreign_items_in_every_placement_builder() {
    let entry = registered("minecraft:stone");
    for (window, name) in [
        (
            protocol::WINDOW_TYPE_GRINDSTONE,
            ContainerName::GrindstoneInput,
        ),
        (
            protocol::WINDOW_TYPE_GRINDSTONE,
            ContainerName::GrindstoneAdditional,
        ),
        (protocol::WINDOW_TYPE_LOOM, ContainerName::LoomInput),
    ] {
        let target = target(name);
        for builder in 0..5 {
            let mut ledger = source(window, &entry, Arc::from([]));
            if builder == 2 {
                let InventoryTarget::Craft(slot) = target else {
                    unreachable!()
                };
                let valid = registered(if window == protocol::WINDOW_TYPE_LOOM {
                    "minecraft:banner"
                } else {
                    "minecraft:diamond_sword"
                });
                ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
                    identity: protocol::SlotIdentity {
                        container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                        slot: u16::from(slot),
                    },
                    stack: NetworkItemStack {
                        network_id: valid.network_id,
                        stack_network_id: 700,
                        count: 1,
                        ..NetworkItemStack::empty()
                    },
                    storage_item: None,
                }));
            }
            if builder == 3 {
                let _ = ledger
                    .begin_hotbar_swap(InventoryTarget::Player(SOURCE_SLOT), 0)
                    .unwrap();
            } else {
                ledger.begin_click(SOURCE_SLOT).unwrap();
            }
            let before = ledger.target_stack(target).cloned();
            let queued = ledger.pending_request_count();
            let cursor = ledger.cursor_stack().cloned();
            let hotbar = ledger.displayed_stack(0).cloned();
            match builder {
                0 => {
                    let _ = ledger.begin_target_gesture(target, CellGesture::Click);
                }
                1 => {
                    let _ = ledger.begin_target_gesture(target, CellGesture::PlaceCount(1));
                }
                2 => {
                    let _ = ledger.begin_target_gesture(target, CellGesture::Click);
                }
                3 => {
                    let _ = ledger.begin_hotbar_swap(target, 0);
                }
                4 => {
                    let _ = ledger.advance_distribute(
                        &mut None,
                        &[target],
                        super::super::DistributeMode::One,
                    );
                }
                _ => unreachable!(),
            }
            assert_eq!(
                ledger.target_stack(target),
                before.as_ref(),
                "{name:?}, builder {builder}"
            );
            assert_eq!(ledger.pending_request_count(), queued);
            assert_eq!(ledger.cursor_stack(), cursor.as_ref());
            assert_eq!(ledger.displayed_stack(0), hotbar.as_ref());
        }
    }
}

/// Empty enchantment lists and typed Unbreakable data have independent admission semantics.
#[test]
fn station_inputs_grindstone_checks_enchantment_list_before_damageability() {
    for (identifier, data, allowed) in [
        ("minecraft:stone", Arc::from([]), false),
        (
            "minecraft:stone",
            extra(&[(9, "ench", &[10, 0, 0, 0, 0])]),
            true,
        ),
        ("minecraft:stone", extra(&[(1, "ench", &[1])]), false),
        ("minecraft:diamond_sword", Arc::from([]), true),
        (
            "minecraft:diamond_sword",
            extra(&[(1, "Unbreakable", &[1])]),
            false,
        ),
        (
            "minecraft:diamond_sword",
            extra(&[(3, "Unbreakable", &1_i32.to_le_bytes())]),
            true,
        ),
        (
            "minecraft:stone",
            extra(&[(9, "ench", &[10, 0, 0, 0, 0]), (1, "Unbreakable", &[1])]),
            true,
        ),
    ] {
        let entry = registered(identifier);
        for name in [
            ContainerName::GrindstoneInput,
            ContainerName::GrindstoneAdditional,
        ] {
            let mut ledger = source(protocol::WINDOW_TYPE_GRINDSTONE, &entry, data.clone());
            ledger.begin_click(SOURCE_SLOT).unwrap();
            let result = ledger.begin_target_gesture(target(name), CellGesture::Click);
            assert_eq!(result.is_ok(), allowed, "{identifier}, {name:?}");
            assert_eq!(ledger.target_stack(target(name)).is_some(), allowed);
        }
    }
}

/// Only a nonzero root Int marks a banner as a special loom input.
#[test]
fn station_inputs_loom_distinguishes_typed_banner_type() {
    let entry = registered("minecraft:banner");
    for (data, allowed) in [
        (Arc::from([]), true),
        (extra(&[(3, "Type", &0_i32.to_le_bytes())]), true),
        (extra(&[(3, "Type", &1_i32.to_le_bytes())]), false),
        (extra(&[(3, "Type", &(-1_i32).to_le_bytes())]), false),
        (extra(&[(1, "Type", &[1])]), true),
        (extra(&[(2, "Type", &1_i16.to_le_bytes())]), true),
    ] {
        let mut ledger = source(protocol::WINDOW_TYPE_LOOM, &entry, data);
        ledger.begin_click(SOURCE_SLOT).unwrap();
        let result =
            ledger.begin_target_gesture(target(ContainerName::LoomInput), CellGesture::Click);
        assert_eq!(result.is_ok(), allowed);
    }
}

/// A rejected station input still quick-moves within the player's inventory.
#[test]
fn station_inputs_quick_move_keeps_ineligible_stacks_in_player_inventory() {
    for (window, entry, data) in [
        (
            protocol::WINDOW_TYPE_GRINDSTONE,
            registered("minecraft:stone"),
            Arc::from([]),
        ),
        (
            protocol::WINDOW_TYPE_LOOM,
            registered("minecraft:banner"),
            extra(&[(3, "Type", &1_i32.to_le_bytes())]),
        ),
        (
            protocol::WINDOW_TYPE_LOOM,
            tagged_entry("custom:standard", "minecraft:banner", None),
            Arc::from([]),
        ),
    ] {
        let mut ledger = source(window, &entry, data);
        let before = ledger.displayed_stack(SOURCE_SLOT).unwrap().clone();
        ledger
            .begin_quick_move(InventoryTarget::Player(SOURCE_SLOT))
            .unwrap();
        let moved = ledger
            .displayed_stack(0)
            .expect("fallback hotbar destination");
        assert_eq!(moved.network_id, before.network_id);
        assert_eq!(moved.extra_data, before.extra_data);
        assert!(ledger.displayed_stack(SOURCE_SLOT).is_none());
    }
}

/// The banner identity survives component declarations, while a tag alone does not grant it.
#[test]
fn station_inputs_loom_uses_item_identity_instead_of_a_banner_tag() {
    for (entry, allowed) in [
        (
            tagged_entry("custom:standard", "minecraft:banner", None),
            false,
        ),
        (
            tagged_entry(assets::facts::BANNER_IDENTIFIER, "minecraft:is_food", None),
            true,
        ),
    ] {
        let mut ledger = source(protocol::WINDOW_TYPE_LOOM, &entry, Arc::from([]));
        ledger.begin_click(SOURCE_SLOT).unwrap();
        let result =
            ledger.begin_target_gesture(target(ContainerName::LoomInput), CellGesture::Click);
        assert_eq!(result.is_ok(), allowed);
    }
}

/// Registry identity replacement retires old predictions; fresh contents use the new identity.
#[test]
fn station_inputs_loom_route_refresh_tracks_identity_changes() {
    let native = registered(assets::facts::BANNER_IDENTIFIER);
    let mut renamed = native.clone();
    renamed.identifier = "custom:standard".into();
    renamed.item_tags = [Arc::from("minecraft:banner")].into();
    for (before, after, old_target, new_target) in [
        (
            &native,
            &renamed,
            target(ContainerName::LoomInput),
            InventoryTarget::Player(0),
        ),
        (
            &renamed,
            &native,
            InventoryTarget::Player(0),
            target(ContainerName::LoomInput),
        ),
    ] {
        for transported in [false, true] {
            let mut ledger = station_ledger(before, protocol::WINDOW_TYPE_LOOM);
            assert_quick_move(&mut ledger, before, old_target);
            if transported {
                assert!(ledger.mark_transport_enqueued(0));
            }
            resend(&mut ledger, after);
            assert_eq!(ledger.pending_request_count(), 0);
            if transported {
                assert!(ledger.resync_required());
            }
            let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
            slots[usize::from(SOURCE_SLOT)] = NetworkItemStack {
                network_id: after.network_id,
                stack_network_id: 801,
                count: 1,
                ..NetworkItemStack::empty()
            };
            ledger.apply(&content(ContainerIdentity::window(0), slots));
            ledger.apply(&content(
                ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT],
            ));
            assert!(!ledger.resync_required());
            assert_quick_move(&mut ledger, after, new_target);
        }
    }
}

/// Builds a custom entry with an optional typed maximum that fits one network VarInt byte.
fn durability_entry(maximum: Option<u8>) -> ItemRegistryEntry {
    let mut bytes = vec![10, 0, 10];
    nbt_string(&mut bytes, "components");
    if let Some(maximum) = maximum {
        bytes.push(10);
        nbt_string(&mut bytes, "minecraft:durability");
        bytes.push(3);
        nbt_string(&mut bytes, "max_durability");
        let encoded = maximum
            .checked_mul(2)
            .filter(|encoded| *encoded < 0x80)
            .expect("one-byte fixture maximum");
        bytes.extend([encoded, 0]);
    }
    bytes.extend([0, 0]);
    protocol::test_support::items::component_item_registry_payload("fixture:durable", 10_002, bytes)
        .entries[0]
        .clone()
}

/// A valid zero maximum still admits both grindstone inputs without inventing a durability bar.
#[test]
fn station_inputs_zero_maximum_component_is_damageable() {
    let entry = durability_entry(Some(0));
    for name in [
        ContainerName::GrindstoneInput,
        ContainerName::GrindstoneAdditional,
    ] {
        for unbreakable in [false, true] {
            let data = if unbreakable {
                extra(&[(1, "Unbreakable", &[1])])
            } else {
                Arc::from([])
            };
            let mut ledger = source(protocol::WINDOW_TYPE_GRINDSTONE, &entry, data);
            assert_eq!(
                ledger
                    .item_facts()
                    .get(entry.network_id)
                    .durability
                    .maximum(),
                None
            );
            ledger.begin_click(SOURCE_SLOT).unwrap();
            let result = ledger.begin_target_gesture(target(name), CellGesture::Click);
            assert_eq!(result.is_ok(), !unbreakable);
            assert_eq!(ledger.target_stack(target(name)).is_some(), !unbreakable);
        }
    }
}

/// Registry permission changes retire every queued placement that used the removed admission.
#[test]
fn station_inputs_admission_refresh_retires_grindstone_placements() {
    let entry = durability_entry(Some(50));
    let replacement = durability_entry(None);
    let target = target(ContainerName::GrindstoneInput);
    for quick_move in [false, true] {
        for transported in [false, true] {
            let mut ledger = source(protocol::WINDOW_TYPE_GRINDSTONE, &entry, Arc::from([]));
            if quick_move {
                ledger
                    .begin_quick_move(InventoryTarget::Player(SOURCE_SLOT))
                    .unwrap();
            } else {
                ledger.begin_click(SOURCE_SLOT).unwrap();
                ledger
                    .begin_target_gesture(target, CellGesture::Click)
                    .unwrap();
            }
            assert!(ledger.target_stack(target).is_some());
            if transported {
                send_pending(&mut ledger);
            }
            resend(&mut ledger, &replacement);
            assert!(ledger.target_stack(target).is_none());
            assert_eq!(ledger.resync_required(), transported);
            if !transported {
                if quick_move {
                    assert_eq!(ledger.displayed_stack(SOURCE_SLOT).unwrap().count, 1);
                    assert!(ledger.cursor_stack().is_none());
                    assert!(ledger.displayed_stack(0).is_none());
                } else {
                    assert!(ledger.displayed_stack(SOURCE_SLOT).is_none());
                    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
                }
            }
        }
    }
}

/// Replacing a maximum with another valid maximum keeps the station permission and request.
#[test]
fn station_inputs_admission_refresh_keeps_valid_grindstone_placements() {
    let entry = durability_entry(Some(50));
    let replacement = durability_entry(Some(60));
    let mut ledger = source(protocol::WINDOW_TYPE_GRINDSTONE, &entry, Arc::from([]));
    ledger.begin_click(SOURCE_SLOT).unwrap();
    ledger
        .begin_target_gesture(target(ContainerName::GrindstoneInput), CellGesture::Click)
        .unwrap();
    let queued = ledger.pending_request_count();
    resend(&mut ledger, &replacement);
    assert_eq!(ledger.pending_request_count(), queued);
    assert!(
        ledger
            .target_stack(target(ContainerName::GrindstoneInput))
            .is_some()
    );
    assert!(!ledger.resync_required());
}

/// Admits every currently unsent scope, leaving response correlation outstanding.
fn send_pending(ledger: &mut PlayerInventoryLedger) {
    while ledger.pending_batch().unwrap().is_some() {
        assert!(ledger.mark_transport_enqueued(0));
    }
}

/// Losing insertion permission does not cancel a gesture that only removes the station item.
#[test]
fn station_inputs_admission_refresh_preserves_grindstone_removal() {
    let entry = durability_entry(Some(50));
    let replacement = durability_entry(None);
    let target = target(ContainerName::GrindstoneInput);
    let InventoryTarget::Craft(slot) = target else {
        unreachable!()
    };
    let mut ledger = source(protocol::WINDOW_TYPE_GRINDSTONE, &entry, Arc::from([]));
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slot: u16::from(slot),
        },
        stack: NetworkItemStack {
            network_id: entry.network_id,
            stack_network_id: 701,
            count: 1,
            ..NetworkItemStack::empty()
        },
        storage_item: None,
    }));
    ledger
        .begin_target_gesture(target, CellGesture::Click)
        .unwrap();
    resend(&mut ledger, &replacement);
    assert_eq!(ledger.pending_request_count(), 1);
    assert!(ledger.cursor_stack().is_some());
    assert!(ledger.target_stack(target).is_none());
    assert!(!ledger.resync_required());
}

/// Gaining station admission retires an earlier fallback move whose route has changed.
#[test]
fn station_inputs_admission_refresh_retires_obsolete_grindstone_fallback() {
    let entry = durability_entry(None);
    let replacement = durability_entry(Some(50));
    for transported in [false, true] {
        let mut ledger = source(protocol::WINDOW_TYPE_GRINDSTONE, &entry, Arc::from([]));
        ledger
            .begin_quick_move(InventoryTarget::Player(SOURCE_SLOT))
            .unwrap();
        assert!(ledger.displayed_stack(0).is_some());
        if transported {
            send_pending(&mut ledger);
        }
        resend(&mut ledger, &replacement);
        assert_eq!(ledger.pending_request_count(), 0);
        assert_eq!(ledger.resync_required(), transported);
        if !transported {
            assert!(ledger.displayed_stack(SOURCE_SLOT).is_some());
            assert!(ledger.displayed_stack(0).is_none());
        }
    }
}

/// A merge is revalidated against its incoming alias even when the destination retains another ID.
#[test]
fn station_inputs_refresh_rechecks_incoming_merge_alias() {
    let entry = durability_entry(Some(50));
    let replacement = durability_entry(None);
    let mut alias = entry.clone();
    alias.network_id += 1;
    let target = target(ContainerName::GrindstoneInput);
    let InventoryTarget::Craft(slot) = target else {
        unreachable!()
    };
    for transported in [false, true] {
        let mut ledger = source(protocol::WINDOW_TYPE_GRINDSTONE, &entry, Arc::from([]));
        let mut entries: Vec<_> = ledger
            .negotiated_item_registry()
            .unwrap()
            .values()
            .cloned()
            .collect();
        entries.push(alias.clone());
        ledger.apply_registry(&ItemRegistryEvent {
            entries: entries.into(),
        });
        let mut incoming = ledger.displayed_stack(SOURCE_SLOT).unwrap().clone();
        incoming.stack_network_id = 201;
        ledger.apply(&content(
            ContainerIdentity::window(0),
            vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT],
        ));
        for (slot, stack) in [
            (0, incoming.clone()),
            (
                u16::from(slot),
                NetworkItemStack {
                    network_id: alias.network_id,
                    stack_network_id: 202,
                    ..incoming
                },
            ),
        ] {
            ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                    slot,
                },
                stack,
                storage_item: None,
            }));
        }
        ledger
            .begin_target_gesture(target, CellGesture::Click)
            .unwrap();
        let merged = ledger.target_stack(target).unwrap();
        assert_eq!((merged.network_id, merged.count), (alias.network_id, 2));
        if transported {
            send_pending(&mut ledger);
        }
        resend(&mut ledger, &replacement);
        assert_eq!(ledger.pending_request_count(), 0);
        assert_eq!(ledger.target_stack(target).unwrap().count, 1);
        assert_eq!(ledger.resync_required(), transported);
    }
}
