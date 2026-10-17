use super::*;
use sha2::{Digest, Sha256};

mod accepted_descendants;

/// Builds a named stack with unrelated NBT that must survive sparse corrections.
fn named_stack(
    id: i32,
    count: u16,
    damage: i32,
    name: &str,
    filtered: Option<&str>,
) -> NetworkItemStack {
    let mut held = stack(6, id, count);
    held.extra_data = response_item_data(damage, name, filtered);
    held.nbt_digest = Sha256::digest(&held.extra_data).into();
    held
}

/// Applies one mining correction, including optional name fields.
fn mining_reply(
    ledger: &mut PlayerInventoryLedger,
    request: i32,
    name: &str,
    filtered: &str,
    id: i32,
    damage: i32,
) {
    let mut reply = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 1, id);
    let slot = &mut Arc::make_mut(&mut reply.slots)[0];
    slot.custom_name = Arc::from(name);
    slot.filtered_custom_name = Arc::from(filtered);
    slot.durability_correction = damage;
    respond(ledger, request, StackResponseStatus::Accepted, vec![reply]);
}

/// Unstated fields preserve the last accepted correction in either response order.
#[test]
fn outstanding_mining_preserves_accepted_identity() {
    for reversed in [false, true] {
        for explicit_later in [false, true] {
            let mut ledger =
                open_ledger(&[(0, named_stack(10, 1, 5, "Old", Some("Filtered old")))]);
            let first = ledger.begin_mining_request(0, 6).unwrap();
            let second = ledger.begin_mining_request(0, 7).unwrap();
            let (first, second) = if reversed {
                (second, first)
            } else {
                (first, second)
            };
            mining_reply(&mut ledger, first, "Accepted", "Filtered accepted", 21, 6);
            assert_eq!(
                ledger.displayed_stack(0),
                Some(&named_stack(
                    21,
                    1,
                    6,
                    "Accepted",
                    Some("Filtered accepted")
                ))
            );
            let (name, filtered) = if explicit_later {
                ("Later", "")
            } else {
                ("", "Ignored")
            };
            mining_reply(&mut ledger, second, name, filtered, 22, 7);
            let (name, filtered) = if explicit_later {
                ("Later", None)
            } else {
                ("Accepted", Some("Filtered accepted"))
            };
            assert_eq!(
                ledger.displayed_stack(0),
                Some(&named_stack(22, 1, 7, name, filtered)),
                "reversed={reversed}, explicit_later={explicit_later}"
            );
            assert_eq!(
                ledger.slot_overlay(0),
                Some(&StackResponseOverlay {
                    custom_name: Some(Arc::from(name)),
                    filtered_custom_name: Some(Arc::from(filtered.unwrap_or(""))),
                    durability_correction: Some(7),
                })
            );
        }
    }
}

/// Rejection and replacement cannot import a stale mining snapshot into a different stack.
#[test]
fn mining_identity_does_not_cross_rejection_or_replacement() {
    for replace in [false, true] {
        let original = named_stack(10, 1, 5, "Old", Some("Filtered old"));
        let mut ledger = open_ledger(&[(0, original.clone())]);
        let first = ledger.begin_mining_request(0, 6).unwrap();
        let second = ledger.begin_mining_request(0, 7).unwrap();
        if replace {
            mining_reply(&mut ledger, first, "Accepted", "Filtered accepted", 21, 6);
            let replacement = named_stack(99, 2, 3, "Replacement", None);
            ledger.apply(&slot_push(0, replacement.clone()));
            mining_reply(&mut ledger, second, "Stale", "Filtered stale", 22, 7);
            assert_eq!(ledger.displayed_stack(0), Some(&replacement));
            assert!(ledger.slot_overlay(0).is_none());
        } else {
            respond(&mut ledger, first, StackResponseStatus::Rejected, vec![]);
            mining_reply(&mut ledger, second, "", "Ignored", 22, 7);
            assert_eq!(
                ledger.displayed_stack(0),
                Some(&named_stack(22, 1, 7, "Old", Some("Filtered old")))
            );
            assert_eq!(ledger.slot_overlay(0).unwrap().custom_name, None);
        }
    }
}

/// Opens a server station without changing retained UI ingredients.
fn station(ledger: &mut PlayerInventoryLedger, id: i32, window_type: i8) {
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(id),
        window_type,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
}

/// Writes one raw UI cell through real packet admission.
fn ui_write(ledger: &mut PlayerInventoryLedger, slot: u8, held: NetworkItemStack) {
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slot: u16::from(slot),
        },
        stack: held,
        storage_item: None,
    }));
}

/// Creates a workbench with an authoritative ingredient and disposable result preview.
fn workbench_preview() -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        protocol::ITEM_DEFAULT_MAX_STACK_SIZE,
        0,
    ));
    ledger.apply(&player_content(&[]));
    ledger.apply(&cursor_content(NetworkItemStack::empty()));
    station(&mut ledger, 1, WORKBENCH_WINDOW_TYPE);
    let input = *protocol::CRAFTING_INPUT_SLOTS.start() + PERSONAL_INPUT_COUNT as u8;
    ui_write(
        &mut ledger,
        input,
        named_stack(31, 2, 0, "Ingredient", None),
    );
    ui_write(
        &mut ledger,
        protocol::CREATED_OUTPUT_SLOT,
        named_stack(32, 1, 0, "Workbench result", None),
    );
    ledger
}

/// An overtaking close retires previews while retaining unacknowledged ingredients.
#[test]
fn close_ack_retires_result_before_another_station() {
    let mut ledger = workbench_preview();
    let input = *protocol::CRAFTING_INPUT_SLOTS.start() + PERSONAL_INPUT_COUNT as u8;
    let ingredient = ledger
        .target_stack(InventoryTarget::Craft(input))
        .unwrap()
        .clone();
    ledger.request_storage_close();
    let cleanup = ledger.queue.back().unwrap().request_id;
    send_all(&mut ledger);
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(1),
        window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(input)),
        Some(&ingredient)
    );
    assert!(
        ledger.created_output_stack().is_none(),
        "retired result must disappear at CloseAck"
    );
    station(&mut ledger, 3, protocol::WINDOW_TYPE_ANVIL);
    respond(
        &mut ledger,
        cleanup,
        StackResponseStatus::Accepted,
        vec![
            correction(
                protocol::ui_slot_container_name(input).unwrap(),
                input,
                0,
                0,
            ),
            correction(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                ingredient.count as u8,
                ingredient.stack_network_id,
            ),
            correction(
                protocol::CONTAINER_NAME_CREATED_OUTPUT,
                protocol::CREATED_OUTPUT_SLOT,
                1,
                44,
            ),
        ],
    );
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(input)),
        Some(&ingredient)
    );
    assert!(ledger.created_output_stack().is_none());
    assert!(
        ledger.displayed_stack(0).is_none(),
        "retired return cannot publish its destination"
    );
    assert_eq!(
        ledger.begin_screen_output(&ScreenCraft::Anvil {
            rename: None,
            multi_recipe_id: 1,
            output: None,
            material_used: 0
        }),
        Err(InventoryGestureError::EmptyGesture)
    );
}

/// Results remain admissible during their own screen generation.
#[test]
fn same_generation_result_is_admissible() {
    let mut ledger = workbench_preview();
    station(&mut ledger, 3, protocol::WINDOW_TYPE_ANVIL);
    let preview = named_stack(41, 1, 0, "Anvil result", None);
    ui_write(&mut ledger, protocol::CREATED_OUTPUT_SLOT, preview.clone());
    assert_eq!(ledger.created_output_stack(), Some(&preview));
    assert!(
        ledger
            .begin_screen_output(&ScreenCraft::Anvil {
                rename: None,
                multi_recipe_id: 1,
                output: None,
                material_used: 0
            })
            .is_ok()
    );
    assert_eq!(
        ledger.cursor_stack().unwrap().extra_data,
        preview.extra_data
    );
}

/// Every screen retirement discards its result independently of retained ingredients.
#[test]
fn result_retirement_preserves_ingredients_across_lifecycle_paths() {
    for path in [
        "replace",
        "server_close",
        "returned",
        "transport",
        "authority",
    ] {
        let mut ledger = workbench_preview();
        let input = *protocol::CRAFTING_INPUT_SLOTS.start() + PERSONAL_INPUT_COUNT as u8;
        let ingredient = ledger
            .target_stack(InventoryTarget::Craft(input))
            .unwrap()
            .clone();
        match path {
            "replace" => station(&mut ledger, 3, protocol::WINDOW_TYPE_ANVIL),
            "server_close" => ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
                container: ContainerIdentity::window(1),
                window_type: WORKBENCH_WINDOW_TYPE,
                server_initiated: true,
            })),
            "returned" => {
                ledger.request_storage_close();
                let cleanup = ledger.queue.back().unwrap().request_id;
                send_all(&mut ledger);
                respond(
                    &mut ledger,
                    cleanup,
                    StackResponseStatus::Accepted,
                    vec![
                        correction(
                            protocol::ui_slot_container_name(input).unwrap(),
                            input,
                            0,
                            0,
                        ),
                        correction(
                            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                            0,
                            ingredient.count as u8,
                            ingredient.stack_network_id,
                        ),
                    ],
                );
                assert_eq!(ledger.displayed_stack(0), Some(&ingredient));
                ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
                    container: ContainerIdentity::window(1),
                    window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
                    server_initiated: false,
                }));
            }
            "transport" => ledger.transport_closed(),
            "authority" => ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client)),
            _ => unreachable!(),
        }
        assert!(ledger.created_output_stack().is_none(), "{path}");
        assert_eq!(
            ledger.target_stack(InventoryTarget::Craft(input)),
            (path != "returned").then_some(&ingredient),
            "{path}"
        );
    }
}

/// A personal CloseAck retires the result without forgetting an unanswered ingredient return.
#[test]
fn personal_close_ack_retires_result_with_retained_ingredient() {
    let mut ledger = open_ledger(&[]);
    let input = *protocol::CRAFTING_INPUT_SLOTS.start();
    let ingredient = named_stack(31, 2, 0, "Ingredient", None);
    ui_write(&mut ledger, input, ingredient.clone());
    ui_write(
        &mut ledger,
        protocol::CREATED_OUTPUT_SLOT,
        named_stack(32, 1, 0, "Personal result", None),
    );
    ledger.request_personal_close();
    send_all(&mut ledger);
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(input)),
        Some(&ingredient)
    );
    assert!(ledger.created_output_stack().is_none());
    assert!(ledger.request_personal_open(42));
    send_all(&mut ledger);
    assert!(ledger.created_output_stack().is_none());
}

/// A newer front-screen gesture cannot roll back a settled HUD name correction.
#[test]
fn mining_identity_survives_later_front_rejection() {
    let mut ledger = open_ledger(&[(0, named_stack(10, 2, 5, "Old", Some("Filtered old")))]);
    let mining = ledger.begin_mining_request(0, 6).unwrap();
    let front = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    mining_reply(&mut ledger, mining, "Accepted", "Filtered accepted", 21, 6);
    respond(&mut ledger, front, StackResponseStatus::Rejected, vec![]);
    assert_eq!(
        ledger.displayed_stack(0),
        Some(&named_stack(
            21,
            1,
            6,
            "Accepted",
            Some("Filtered accepted")
        ))
    );
    assert_eq!(
        ledger.slot_overlay(0).unwrap().custom_name.as_deref(),
        Some("Accepted")
    );
}

/// Starting a personal generation also updates a view folded over unrelated HUD work.
#[test]
fn opening_personal_screen_discards_dormant_result_from_folded_view() {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&player_content(&[(0, named_stack(10, 2, 0, "Held", None))]));
    ui_write(
        &mut ledger,
        protocol::CREATED_OUTPUT_SLOT,
        named_stack(32, 1, 0, "Dormant result", None),
    );
    ledger.begin_world_drop(0, Some(1)).unwrap();
    assert!(ledger.created_output_stack().is_some());
    assert!(ledger.request_personal_open(42));
    assert!(ledger.created_output_stack().is_none());
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 1);
}

/// Keeping another screen's ingredients cannot keep the closing screen's result alive.
#[test]
fn immediate_close_discards_result_with_only_inactive_ingredients() {
    let mut ledger = workbench_preview();
    let inactive = *protocol::CRAFTING_INPUT_SLOTS.start();
    let active = inactive + PERSONAL_INPUT_COUNT as u8;
    ui_write(&mut ledger, active, NetworkItemStack::empty());
    let ingredient = named_stack(31, 2, 0, "Inactive ingredient", None);
    ui_write(&mut ledger, inactive, ingredient.clone());
    ledger.request_storage_close();
    assert!(ledger.created_output_stack().is_none());
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(inactive)),
        Some(&ingredient)
    );
    assert!(!ledger.resync_required());
}

/// Signed response damage replaces the full retained integer, including equal low bits.
#[test]
fn accepted_damage_correction_materializes_full_width_retained_values() {
    for (previous, wire, expected) in [
        (65_535, -1, -1),
        (65_536, 0, 0),
        (32_768, -32_768, -32_768),
        (i32::MAX, 65_535, -1),
        (i32::MIN, 65_536, 0),
        (7, 65_543, 7),
    ] {
        for mining in [false, true] {
            let mut ledger =
                open_ledger(&[(0, named_stack(10, 4, previous, "Old", Some("Filtered old")))]);
            let (request, count) = if mining {
                (ledger.begin_mining_request(0, 0).unwrap(), 4)
            } else {
                let request = ledger
                    .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
                    .unwrap();
                send_all(&mut ledger);
                (request, 3)
            };
            let mut reply = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, count, 21);
            Arc::make_mut(&mut reply.slots)[0].durability_correction = wire;
            respond(
                &mut ledger,
                request,
                StackResponseStatus::Accepted,
                vec![reply],
            );
            let retained = ledger.displayed_stack(0).unwrap();
            assert_eq!(
                retained,
                &named_stack(21, u16::from(count), expected, "Old", Some("Filtered old")),
                "previous={previous}, wire={wire}, mining={mining}"
            );
            assert_eq!(
                protocol::item_extra_damage_value(&retained.extra_data),
                Some(expected)
            );
            assert_eq!(
                protocol::item_stack_damage(retained),
                u32::try_from(expected).ok()
            );
            assert_eq!(
                ledger.slot_overlay(0).unwrap().durability_correction,
                (expected >= 0).then_some(expected)
            );
        }
    }
}
