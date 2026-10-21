use inventory::PlayerInventoryLedger;
use inventory::inventory_ledger::{
    CellGesture, DropSource, InventoryGestureError, InventoryTarget,
};
use protocol::*;
use sha2::{Digest, Sha256};

/// Builds valid original user data with a typed item-lock value.
fn locked(lock: u8, tag: u8) -> NetworkItemStack {
    let key = b"minecraft:item_lock";
    let mut extra = vec![255, 255, 1, 10, 0, 0, tag];
    extra.extend_from_slice(&(key.len() as u16).to_le_bytes());
    extra.extend_from_slice(key);
    extra.push(lock);
    if tag == 2 {
        extra.push(0);
    }
    extra.push(0);
    extra.extend_from_slice(&[0; 8]);
    NetworkItemStack {
        network_id: 6,
        stack_network_id: 41,
        count: 4,
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..NetworkItemStack::empty()
    }
}

/// Opens an observed personal inventory containing the exact source stack.
#[expect(
    clippy::print_stderr,
    reason = "a missing local fixture must report why the test skips"
)]
fn ready(
    slot: u8,
    stack: NetworkItemStack,
    cursor: bool,
    personal: bool,
) -> Option<PlayerInventoryLedger> {
    if assets::facts::ItemFacts::builtin().is_err() {
        eprintln!("missing-fixture: builtin item facts carrier");
        return None;
    }
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: vec![ItemRegistryEntry {
            identifier: "minecraft:apple".into(),
            network_id: 6,
            component_based: false,
            version: ItemRegistryVersion::Legacy,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: [].into(),
        }]
        .into(),
    });
    let mut slots =
        vec![NetworkItemStack::empty(); inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT];
    if !cursor {
        slots[usize::from(slot)] = stack.clone();
    }
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(UI_INVENTORY_WINDOW_ID),
            slot: 0,
        },
        stack: if cursor {
            stack
        } else {
            NetworkItemStack::empty()
        },
        storage_item: None,
    }));
    if personal {
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
    }
    Some(ledger)
}

#[test]
fn slot_locks_apply_to_player_removal_and_special_actions() {
    for slot in [0, 9] {
        for lock in [0, 1, 2] {
            for route in 0..6 {
                let Some(mut ledger) = ready(slot, locked(lock, 1), false, true) else {
                    return;
                };
                let target = InventoryTarget::Player(slot);
                let result = match route {
                    0 => ledger.begin_target_gesture(target, CellGesture::Click),
                    1 => ledger.begin_take_count(slot, 2),
                    2 => ledger.begin_hotbar_swap(target, 1),
                    3 => ledger.begin_quick_move(target),
                    4 => ledger.begin_drop(DropSource::Target(target), Some(1)),
                    _ => ledger.begin_world_drop(slot, Some(1)),
                };
                let refused = lock == 1 || (route >= 4 && lock != 0);
                if refused {
                    assert_eq!(
                        result,
                        Err(InventoryGestureError::SlotLocked),
                        "slot={slot} lock={lock} route={route}"
                    );
                    assert_eq!(ledger.displayed_stack(slot), Some(&locked(lock, 1)));
                    assert_eq!(ledger.pending_request_count(), 0);
                } else {
                    assert!(
                        result.is_ok(),
                        "slot={slot} lock={lock} route={route}: {result:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn special_removal_checks_nonzero_cursor_locks() {
    for lock in [1, 2] {
        for destroy in [false, true] {
            let Some(mut ledger) = ready(0, locked(lock, 1), true, true) else {
                return;
            };
            let result = if destroy {
                ledger.begin_destroy_cursor(None)
            } else {
                ledger.begin_drop(DropSource::Cursor, Some(1))
            };
            assert_eq!(result, Err(InventoryGestureError::SlotLocked));
            assert_eq!(ledger.cursor_stack(), Some(&locked(lock, 1)));
        }
    }
}

#[test]
fn slot_locks_require_the_byte_tag_type() {
    let Some(mut ledger) = ready(0, locked(1, 2), false, true) else {
        return;
    };
    assert!(ledger.begin_click(0).is_ok());
}

/// Opens an empty chest around the same authoritative player source used by removal tests.
fn storage_ready(lock: u8, tag: u8) -> Option<PlayerInventoryLedger> {
    let mut ledger = ready(0, locked(lock, tag), false, false)?;
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(5),
        window_type: 0,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(5),
        slots: vec![NetworkItemStack::empty(); 27].into(),
        storage_item: NetworkItemStack::empty(),
    }));
    Some(ledger)
}

#[test]
fn destination_locks_cover_place_quick_move_and_both_swap_directions() {
    for (lock, tag) in [(0, 1), (2, 1), (2, 2)] {
        for route in 0..4 {
            let Some(mut ledger) = storage_ready(lock, tag) else {
                return;
            };
            let result = match route {
                0 => {
                    ledger.begin_click(0).unwrap();
                    ledger.begin_target_gesture(
                        InventoryTarget::Storage(0),
                        CellGesture::PlaceCount(1),
                    )
                }
                1 => ledger.begin_quick_move(InventoryTarget::Player(0)),
                2 => ledger.begin_hotbar_swap(InventoryTarget::Storage(0), 0),
                _ => {
                    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                        identity: SlotIdentity {
                            container: ContainerIdentity::window(5),
                            slot: 0,
                        },
                        stack: NetworkItemStack {
                            stack_network_id: 51,
                            metadata: 1,
                            ..locked(0, 1)
                        },
                        storage_item: None,
                    }));
                    ledger.begin_hotbar_swap(InventoryTarget::Storage(0), 0)
                }
            };
            if lock != 0 && tag == 1 {
                assert_eq!(
                    result,
                    Err(InventoryGestureError::SlotLocked),
                    "route={route}"
                );
                assert_eq!(
                    ledger
                        .target_stack(InventoryTarget::Storage(0))
                        .map(|stack| stack.metadata),
                    (route == 3).then_some(1)
                );
            } else {
                assert!(
                    result.is_ok(),
                    "lock={lock} tag={tag} route={route}: {result:?}"
                );
            }
        }
    }
}

#[test]
fn consume_refuses_every_nonzero_typed_item_lock() {
    for lock in [0, 1, 2] {
        let Some(mut ledger) = ready(0, locked(0, 1), false, false) else {
            return;
        };
        ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(5),
            window_type: WINDOW_TYPE_STONECUTTER,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(UI_INVENTORY_WINDOW_ID),
                slot: 3,
            },
            stack: locked(lock, 1),
            storage_item: None,
        }));
        let result =
            ledger.begin_screen_output(&inventory::inventory_ledger::ScreenCraft::Predicted {
                recipe_network_id: 44,
                output: RecipeOutput {
                    network_id: 6,
                    aux: 0,
                    count: 1,
                    block_runtime_id: 0,
                    empty_envelope: true,
                },
            });
        if lock == 0 {
            assert!(result.is_ok(), "unlocked control: {result:?}");
        } else {
            assert_eq!(result, Err(InventoryGestureError::SlotLocked));
            assert_eq!(ledger.pending_request_count(), 0);
            assert_eq!(
                ledger
                    .target_stack(InventoryTarget::Craft(3))
                    .unwrap()
                    .count,
                4
            );
        }
    }
}

/// Builds ordinary binding NBT using the public full-width editor.
fn binding(level: i16) -> NetworkItemStack {
    let mut held = locked(0, 1);
    held.count = 1;
    let id = i16::from(protocol_ids::Enchantment::Curseofbinding.wire_value());
    held.extra_data = edit_item_extra(
        &held.extra_data,
        &ItemNbtEdit {
            enchantments: Some(vec![(id, level)]),
            ..Default::default()
        },
    )
    .unwrap();
    held.nbt_digest = Sha256::digest(&held.extra_data).into();
    held
}

#[test]
fn binding_armor_removal_uses_actor_mode_independently_of_instabuild() {
    use inventory::inventory_ledger::InventoryPermissions;
    for creative in [false, true] {
        for instabuild in [None, Some(false), Some(true)] {
            for level in [0, 1, 256] {
                for route in 0..4 {
                    let Some(mut ledger) = ready(0, locked(0, 1), false, true) else {
                        return;
                    };
                    ledger.set_permissions(InventoryPermissions {
                        creative,
                        instabuild,
                    });
                    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                        identity: SlotIdentity {
                            container: ContainerIdentity::window(0),
                            slot: 0,
                        },
                        stack: NetworkItemStack::empty(),
                        storage_item: None,
                    }));
                    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                        container: ContainerIdentity::window(ARMOR_WINDOW_ID),
                        slots: [
                            binding(level),
                            NetworkItemStack::empty(),
                            NetworkItemStack::empty(),
                            NetworkItemStack::empty(),
                        ]
                        .into(),
                        storage_item: NetworkItemStack::empty(),
                    }));
                    let target = InventoryTarget::Armor(0);
                    let result = match route {
                        0 => ledger.begin_target_gesture(target, CellGesture::Click),
                        1 => ledger.begin_hotbar_swap(target, 0),
                        2 => ledger.begin_quick_move(target),
                        _ => ledger.begin_drop(DropSource::Target(target), Some(1)),
                    };
                    if !creative && level > 0 {
                        assert_eq!(
                            result,
                            Err(InventoryGestureError::SlotLocked),
                            "instabuild={instabuild:?} level={level} route={route}"
                        );
                        assert_eq!(ledger.pending_request_count(), 0);
                    } else {
                        assert!(
                            result.is_ok(),
                            "creative={creative} instabuild={instabuild:?} level={level} route={route}: {result:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn unrelated_display_data_does_not_disable_binding_removal_checks() {
    let Some(mut ledger) = ready(0, locked(0, 1), false, true) else {
        return;
    };
    let mut held = binding(1);
    let mut data = held.extra_data.to_vec();
    let position = data.len() - 9;
    data.splice(
        position..position,
        [1, 7, 0, b'd', b'i', b's', b'p', b'l', b'a', b'y', 1],
    );
    held.nbt_digest = Sha256::digest(&data).into();
    held.extra_data = data.into();
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(ARMOR_WINDOW_ID),
        slots: [
            held,
            NetworkItemStack::empty(),
            NetworkItemStack::empty(),
            NetworkItemStack::empty(),
        ]
        .into(),
        storage_item: NetworkItemStack::empty(),
    }));
    assert_eq!(
        ledger.begin_target_gesture(InventoryTarget::Armor(0), CellGesture::Click),
        Err(InventoryGestureError::SlotLocked)
    );
}
