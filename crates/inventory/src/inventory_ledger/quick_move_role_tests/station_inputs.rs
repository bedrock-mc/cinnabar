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
