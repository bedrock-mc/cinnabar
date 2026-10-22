//! Workstation result takes: request shapes, shift-click placement and rollback.

use super::*;

/// An open anvil holding a one-item target and a stack of three materials.
fn anvil_with_inputs() -> (PlayerInventoryLedger, NetworkItemStack) {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 5, WINDOW_TYPE_ANVIL);
    ledger.apply(&slot_update(named(124, 0), 1, stack(30, 1)));
    ledger.apply(&slot_update(named(124, 1), 2, stack(31, 3)));
    (ledger, stack(0, 1))
}

/// Rejects the newest request and asserts the anvil, cursor and inventory are as before.
fn reject_and_assert_restored(ledger: &mut PlayerInventoryLedger, request_id: i32) {
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                request_id,
                status: protocol::StackResponseStatus::Rejected,
                containers: Arc::from([]),
            }]),
        },
    ));
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(1)),
        Some(&stack(30, 1))
    );
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(2)),
        Some(&stack(31, 3))
    );
    assert!(
        (0..PLAYER_INVENTORY_SLOT_COUNT as u8).all(|slot| ledger.displayed_stack(slot).is_none())
    );
    assert!(!ledger.resync_required());
    assert!(
        ledger.begin_quick_move(InventoryTarget::Craft(2)).is_ok(),
        "the restored material moves again"
    );
}

/// The anvil take carries no result list (a dedicated server crashes on one), consumes the
/// material before the target, and a rejection restores every cell.
#[test]
fn anvil_take_matches_the_native_request_and_rolls_back_on_rejection() {
    let (mut ledger, result) = anvil_with_inputs();
    let request_id = ledger
        .begin_screen_output(&ScreenCraft::Anvil {
            rename: None,
            recipe_network_id: 0,
            output: Some(result),
            material_used: 2,
        })
        .unwrap();
    let slot = |container, slot, stack_network_id| protocol::StackRequestSlot {
        container,
        slot,
        stack_network_id,
    };
    let window = |name| StackRequestContainer::OpenWindow {
        name,
        dynamic_id: None,
    };
    assert_eq!(
        ledger.newest_request().unwrap().actions,
        vec![
            StackRequestAction::CraftRecipeOptional {
                recipe_network_id: 0,
                filtered_string_index: 0,
            },
            StackRequestAction::Consume {
                amount: 2,
                source: slot(window(1), 2, 31),
            },
            StackRequestAction::Consume {
                amount: 1,
                source: slot(window(0), 1, 30),
            },
            StackRequestAction::Take {
                amount: 1,
                source: slot(
                    StackRequestContainer::CreatedOutput,
                    protocol::CREATED_OUTPUT_SLOT,
                    request_id
                ),
                destination: slot(StackRequestContainer::Cursor, 0, 0),
            },
        ]
    );
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(2))
            .unwrap()
            .count,
        1
    );
    reject_and_assert_restored(&mut ledger, request_id);
}

/// A shift-click places the anvil result into the inventory instead of the cursor.
#[test]
fn anvil_shift_take_places_into_the_inventory_and_rolls_back() {
    let (mut ledger, result) = anvil_with_inputs();
    let request_id = ledger
        .begin_screen_output_into(
            &ScreenCraft::Anvil {
                rename: Some(Arc::from("Named")),
                recipe_network_id: 0,
                output: Some(result),
                material_used: 3,
            },
            CraftSink::Inventory,
        )
        .unwrap();
    let request = ledger.newest_request().unwrap();
    let Some(StackRequestAction::Place {
        amount: 1,
        source,
        destination,
    }) = request.actions.last()
    else {
        panic!("the result is placed: {:?}", request.actions);
    };
    assert_eq!(source.container, StackRequestContainer::CreatedOutput);
    assert_eq!(
        destination.container,
        StackRequestContainer::PlayerInventory
    );
    assert_eq!(destination.stack_network_id, 0);
    assert!(
        request
            .actions
            .iter()
            .all(|action| !matches!(action, StackRequestAction::CraftResultsDeprecated { .. }))
    );
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(destination.slot).unwrap().count, 1);
    assert!(ledger.target_stack(InventoryTarget::Craft(2)).is_none());
    reject_and_assert_restored(&mut ledger, request_id);
}

/// The stonecutter and cartography table derive their results on the server, so their takes
/// send no result list; the cartography table names its map through an optional-recipe craft.
#[test]
fn server_derived_screens_send_no_result_list() {
    for (window_type, input) in [
        (protocol::WINDOW_TYPE_STONECUTTER, 3),
        (protocol::WINDOW_TYPE_CARTOGRAPHY, 12),
        (protocol::WINDOW_TYPE_GRINDSTONE, 16),
    ] {
        let mut ledger = ledger_with(&[]);
        open(&mut ledger, 7, window_type);
        let name = protocol::ui_slot_container_name(input).unwrap();
        ledger.apply(&slot_update(
            named(124, name),
            u16::from(input),
            stack(40, 2),
        ));
        let craft = if window_type == protocol::WINDOW_TYPE_GRINDSTONE {
            ledger.apply(&slot_update(
                ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                u16::from(protocol::CREATED_OUTPUT_SLOT),
                stack(41, 1),
            ));
            ScreenCraft::Grindstone {
                recipe_network_id: 40,
                repair_cost: 0,
                output: None,
            }
        } else {
            let output = protocol::RecipeOutput {
                network_id: 6,
                aux: 0,
                count: 1,
                block_runtime_id: 0,
                empty_envelope: false,
            };
            if window_type == protocol::WINDOW_TYPE_CARTOGRAPHY {
                ScreenCraft::Cartography {
                    recipe_network_id: 9,
                    output,
                    name: Arc::from(""),
                }
            } else {
                ScreenCraft::Predicted {
                    recipe_network_id: 9,
                    output,
                }
            }
        };
        ledger.begin_screen_output(&craft).unwrap();
        let sends_results = ledger
            .newest_request()
            .unwrap()
            .actions
            .iter()
            .any(|action| matches!(action, StackRequestAction::CraftResultsDeprecated { .. }));
        assert_eq!(
            sends_results,
            window_type == protocol::WINDOW_TYPE_GRINDSTONE,
            "window type {window_type}"
        );
        if window_type == protocol::WINDOW_TYPE_CARTOGRAPHY {
            let request = ledger.newest_request().unwrap();
            assert_eq!(
                request.actions[0],
                StackRequestAction::CraftRecipeOptional {
                    recipe_network_id: 9,
                    filtered_string_index: 0,
                }
            );
            assert_eq!(request.filter_strings, [String::new()]);
        }
    }
}

/// The smithing table consumes its template, then the item, then the material.
#[test]
fn smithing_takes_consume_the_template_first() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 7, protocol::WINDOW_TYPE_SMITHING_TABLE);
    for (slot, id) in [(51, 61), (52, 62), (53, 63)] {
        let name = protocol::ui_slot_container_name(slot).unwrap();
        ledger.apply(&slot_update(
            named(124, name),
            u16::from(slot),
            stack(id, 1),
        ));
    }
    ledger
        .begin_screen_output(&ScreenCraft::Predicted {
            recipe_network_id: 9,
            output: protocol::RecipeOutput {
                network_id: 6,
                aux: 0,
                count: 1,
                block_runtime_id: 0,
                empty_envelope: false,
            },
        })
        .unwrap();
    let consumed: Vec<i32> = ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .filter_map(|action| match action {
            StackRequestAction::Consume { source, .. } => Some(source.stack_network_id),
            _ => None,
        })
        .collect();
    assert_eq!(consumed, [63, 61, 62]);
}

/// `stack(id, count)` carrying a custom name.
fn named_stack(stack_network_id: i32, count: u16, name: &str) -> NetworkItemStack {
    let extra = protocol::edit_item_extra(
        &[],
        &protocol::ItemNbtEdit {
            custom_name: Some(Some(name.into())),
            ..Default::default()
        },
    )
    .unwrap();
    NetworkItemStack {
        nbt_digest: sha2::Sha256::digest(&extra).into(),
        extra_data: extra,
        ..stack(stack_network_id, count)
    }
}

/// A shift-clicked named result joins a partial stack with the same name even when every
/// other cell is full, and never one with a different name.
#[test]
fn shift_taken_named_results_merge_into_equal_partial_stacks() {
    let mut slots: Vec<(usize, NetworkItemStack)> = (2..PLAYER_INVENTORY_SLOT_COUNT)
        .map(|slot| (slot, stack(100 + slot as i32, 64)))
        .collect();
    slots.push((0, named_stack(90, 5, "Other")));
    slots.push((1, named_stack(91, 5, "Named")));
    let mut ledger = ledger_with(&slots);
    open(&mut ledger, 5, WINDOW_TYPE_ANVIL);
    ledger.apply(&slot_update(named(124, 0), 1, stack(30, 1)));
    ledger
        .begin_screen_output_into(
            &ScreenCraft::Anvil {
                rename: Some(Arc::from("Named")),
                recipe_network_id: 0,
                output: Some(named_stack(0, 1, "Named")),
                material_used: 0,
            },
            CraftSink::Inventory,
        )
        .unwrap();
    let Some(StackRequestAction::Place { destination, .. }) =
        ledger.newest_request().unwrap().actions.last().cloned()
    else {
        panic!("the result is placed");
    };
    assert_eq!((destination.slot, destination.stack_network_id), (1, 91));
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 6);
}

/// The loom's stand-in banner lacks its pattern, so a shift-click never merges it into a stack
/// of blank banners.
#[test]
fn shift_taken_loom_results_only_fill_empty_cells() {
    let mut ledger = ledger_with(&[(0, stack(90, 3))]);
    open(&mut ledger, 5, protocol::WINDOW_TYPE_LOOM);
    for (slot, id) in [(9, 40), (10, 41)] {
        let name = protocol::ui_slot_container_name(slot).unwrap();
        ledger.apply(&slot_update(
            named(124, name),
            u16::from(slot),
            stack(id, 1),
        ));
    }
    ledger
        .begin_screen_output_into(
            &ScreenCraft::Loom {
                pattern: Arc::from("bri"),
            },
            CraftSink::Inventory,
        )
        .unwrap();
    let Some(StackRequestAction::Place { destination, .. }) =
        ledger.newest_request().unwrap().actions.last().cloned()
    else {
        panic!("the banner is placed");
    };
    assert_eq!((destination.slot, destination.stack_network_id), (1, 0));
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 3);
}

/// A `stack(id, 1)` worn to `damage` and enchanted to `level`, which the grindstone admits.
fn worn(stack_network_id: i32, damage: i32, level: i16) -> NetworkItemStack {
    let extra = protocol::edit_item_extra(
        &[],
        &protocol::ItemNbtEdit {
            damage: Some(damage),
            enchantments: Some(vec![(9, level)]),
            ..Default::default()
        },
    )
    .unwrap();
    NetworkItemStack {
        nbt_digest: sha2::Sha256::digest(&extra).into(),
        extra_data: extra,
        ..stack(stack_network_id, 1)
    }
}

/// A take queued before its input placement is acknowledged is dropped, not sent with a stale
/// result, when the acknowledgement corrects that input's damage or count.
#[test]
fn unsent_takes_are_dropped_when_their_inputs_are_corrected() {
    for (count, damage) in [(1_u8, 1000), (2, 900)] {
        // Different enchantment levels keep the placed copy from stacking onto the first.
        let mut ledger = ledger_with(&[(0, worn(61, 900, 2))]);
        open(&mut ledger, 7, protocol::WINDOW_TYPE_GRINDSTONE);
        let top = protocol::ui_slot_container_name(16).unwrap();
        ledger.apply(&slot_update(named(124, top), 16, worn(50, 900, 1)));
        let placement = ledger.begin_quick_move(InventoryTarget::Player(0)).unwrap();
        assert!(ledger.mark_transport_enqueued(1));
        assert!(ledger.target_stack(InventoryTarget::Craft(17)).is_some());
        let mut stale = worn(0, 161, 0);
        stale.count = 1;
        ledger
            .begin_screen_output(&ScreenCraft::Grindstone {
                recipe_network_id: 50,
                repair_cost: 0,
                output: Some(stale),
            })
            .unwrap();
        assert_eq!(ledger.pending_request_count(), 2);
        ledger.apply(&InventoryEvent::Response(
            protocol::ItemStackResponseEvent {
                responses: Arc::from([protocol::StackResponse {
                    request_id: placement,
                    status: protocol::StackResponseStatus::Accepted,
                    containers: Arc::from([protocol::StackResponseContainer {
                        container: ContainerIdentity {
                            window_id: None,
                            slot_type: protocol::ui_slot_container_name(17),
                            dynamic_id: None,
                        },
                        slots: Arc::from([protocol::StackResponseSlot {
                            slot: 17,
                            hotbar_slot: 17,
                            count,
                            item_stack_id: 61,
                            custom_name: Arc::from(""),
                            filtered_custom_name: Arc::from(""),
                            durability_correction: damage,
                        }]),
                    }]),
                }]),
            },
        ));
        assert_eq!(
            ledger.pending_request_count(),
            0,
            "the stale take is never sent"
        );
        assert!(ledger.cursor_stack().is_none());
        assert_eq!(
            ledger.target_stack(InventoryTarget::Craft(16)),
            Some(&worn(50, 900, 1))
        );
        let corrected = ledger.target_stack(InventoryTarget::Craft(17)).unwrap();
        assert_eq!(
            protocol::item_extra_damage_value(&corrected.extra_data),
            Some(damage)
        );
        assert_eq!(corrected.count, u16::from(count));
    }
}

/// Recipe predictions that lack the result's item data (a named map) only fill empty cells,
/// while a stonecutter output, which is the whole item, still joins an equal partial stack.
#[test]
fn unmaterialized_recipe_results_only_fill_empty_cells() {
    for (window_type, input, materialized) in [
        (protocol::WINDOW_TYPE_CARTOGRAPHY, 12, false),
        (protocol::WINDOW_TYPE_STONECUTTER, 3, true),
    ] {
        let mut ledger = ledger_with(&[(0, stack(90, 3))]);
        open(&mut ledger, 5, window_type);
        let name = protocol::ui_slot_container_name(input).unwrap();
        ledger.apply(&slot_update(
            named(124, name),
            u16::from(input),
            stack(40, 2),
        ));
        let output = protocol::RecipeOutput {
            network_id: 6,
            aux: 0,
            count: 1,
            block_runtime_id: 0,
            empty_envelope: false,
        };
        let craft = if materialized {
            ScreenCraft::Predicted {
                recipe_network_id: 9,
                output,
            }
        } else {
            ScreenCraft::Cartography {
                recipe_network_id: 9,
                output,
                name: Arc::from("Map"),
            }
        };
        ledger
            .begin_screen_output_into(&craft, CraftSink::Inventory)
            .unwrap();
        let Some(StackRequestAction::Place { destination, .. }) =
            ledger.newest_request().unwrap().actions.last().cloned()
        else {
            panic!("the result is placed");
        };
        let expected = if materialized { (0, 90) } else { (1, 0) };
        assert_eq!(
            (destination.slot, destination.stack_network_id),
            expected,
            "window type {window_type}"
        );
    }
}

/// A registry update can change the facts a derived result was predicted from, so it drops an
/// unsent take rather than send that result.
#[test]
fn registry_updates_drop_unsent_derived_takes() {
    let mut ledger = ledger_with(&[]);
    open(&mut ledger, 7, protocol::WINDOW_TYPE_GRINDSTONE);
    let top = protocol::ui_slot_container_name(16).unwrap();
    ledger.apply(&slot_update(named(124, top), 16, worn(50, 90, 1)));
    ledger
        .begin_screen_output(&ScreenCraft::Grindstone {
            recipe_network_id: 50,
            repair_cost: 0,
            output: Some(worn(0, 90, 0)),
        })
        .unwrap();
    assert_eq!(ledger.pending_request_count(), 1);
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        32,
        0,
    ));
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(16)),
        Some(&worn(50, 90, 1))
    );
}
