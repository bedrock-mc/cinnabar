use super::*;

/// Accepts a take and place so a server correction travels back into the target cell.
pub(super) fn correct_target(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    damage: i32,
    name: &str,
) {
    use inventory::inventory_ledger::CellGesture;
    use protocol::{
        ItemStackResponseEvent, StackResponse, StackResponseContainer, StackResponseSlot,
        StackResponseStatus,
    };
    let target = ANVIL_TARGET_SLOT;
    let target_name = protocol::ui_slot_container_name(target).unwrap();
    for taking in [true, false] {
        let request_id = runtime
            .inventory_ledger_mut(player)
            .begin_target_gesture(InventoryTarget::Craft(target), CellGesture::Click)
            .unwrap();
        let containers = [
            (target_name, target, !taking),
            (protocol::CONTAINER_NAME_CURSOR, 0, taking),
        ]
        .map(|(slot_type, slot, occupied)| StackResponseContainer {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(slot_type),
                dynamic_id: None,
            },
            slots: Arc::from([StackResponseSlot {
                slot,
                hotbar_slot: slot,
                count: u8::from(occupied),
                item_stack_id: if occupied { 30 } else { 0 },
                custom_name: Arc::from(if occupied { name } else { "" }),
                filtered_custom_name: Arc::from(""),
                durability_correction: if occupied { damage } else { 0 },
            }]),
        });
        runtime
            .inventory_ledger_mut(player)
            .apply(&InventoryEvent::Response(ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    status: StackResponseStatus::Accepted,
                    request_id,
                    containers: Arc::from(containers),
                }]),
            }));
    }
}

#[test]
fn response_corrections_drive_repair_cost_name_and_retained_nbt() {
    for (damage, material_used) in [(-1, 0), (0, 0), (100, 1), (800, 3)] {
        let (mut runtime, mut player) = anvil();
        runtime.sync_anvil_name(&player);
        correct_target(&mut runtime, &mut player, damage, "Server blade");
        runtime.sync_anvil_name(&player);
        assert_eq!(runtime.screen_state().anvil_name, "Server blade");
        let target = runtime
            .inventory_ledger(&player)
            .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .unwrap();
        assert_eq!(
            protocol::item_extra_damage_value(&target.extra_data),
            Some(damage),
            "the response rewrites the retained damage tag"
        );
        let preview = runtime.anvil_preview(&player);
        if material_used == 0 {
            assert!(
                preview.is_none(),
                "a pristine correction needs no repair or rename"
            );
        } else {
            let preview = preview.unwrap();
            assert_eq!(preview.outcome.material_used, material_used);
            assert_eq!(preview.outcome.cost, i32::from(material_used));
            assert_eq!(
                preview.outcome.custom_name, None,
                "the corrected name is unchanged"
            );
            assert_eq!(
                protocol::item_display(&preview.stack.extra_data)
                    .name
                    .as_deref(),
                Some("Server blade")
            );
        }
    }
}

#[test]
fn enchantment_only_outputs_keep_corrected_damage() {
    let (mut runtime, mut player) = anvil();
    correct_target(&mut runtime, &mut player, 100, "Server blade");
    place_book(&mut runtime, &mut player, vec![(17, 1)]);
    runtime.sync_anvil_name(&player);
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.damage, None, "enchanting does not repair");
    assert_eq!(protocol::item_stack_damage(&preview.stack), Some(100));
}

#[test]
fn unedited_custom_names_with_outer_whitespace_do_not_add_rename_work() {
    let (mut runtime, mut player) = anvil();
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            custom_name: Some(Some(" Blade ".into())),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, " Blade ");
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.cost, 3);
    assert_eq!(preview.outcome.custom_name, None);
    assert_eq!(
        protocol::item_display(&preview.stack.extra_data)
            .name
            .as_deref(),
        Some(" Blade ")
    );
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_MATERIAL_SLOT),
        NetworkItemStack::empty(),
    );
    assert!(
        runtime.anvil_preview(&player).is_none(),
        "placing the named item alone makes no rename"
    );
}

#[test]
fn potion_name_prefill_tracks_the_metadata_and_the_displayed_name() {
    let (mut runtime, mut player) = anvil();
    let input = b"potion.heal.name=Healing\npotion.moveSpeed.name=Swiftness\n";
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input);
        true
    }));
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&ItemRegistryEvent {
            entries: Arc::from([entry("minecraft:potion", SWORD)]),
        });
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_MATERIAL_SLOT),
        NetworkItemStack::empty(),
    );
    for (identifier, name) in [
        ("minecraft:healing", "Healing"),
        ("minecraft:swiftness", "Swiftness"),
    ] {
        let metadata = u32::try_from(
            assets::potion::POTION_DESCRIPTORS
                .iter()
                .position(|potion| potion.identifier == identifier)
                .unwrap(),
        )
        .unwrap();
        place(
            &mut runtime,
            &mut player,
            u16::from(ANVIL_TARGET_SLOT),
            NetworkItemStack {
                network_id: SWORD,
                stack_network_id: 30,
                count: 1,
                metadata,
                ..NetworkItemStack::empty()
            },
        );
        runtime.sync_anvil_name(&player);
        assert_eq!(runtime.screen_state().anvil_name, name);
        assert!(
            runtime.anvil_preview(&player).is_none(),
            "the displayed name is not a rename"
        );
    }
}

#[test]
fn unchanged_preview_is_shared_until_inputs_names_or_rules_change() {
    let (mut runtime, mut player) = anvil();
    runtime.sync_anvil_name(&player);
    let first = runtime.anvil_preview(&player).unwrap();
    for _ in 0..128 {
        runtime.sync_anvil_name(&player);
        let repeated = runtime.anvil_preview(&player).unwrap();
        assert!(Arc::ptr_eq(&first, &repeated));
        let shown = runtime.anvil_output(&player).unwrap();
        assert!(Arc::ptr_eq(&first.stack.extra_data, &shown.extra_data));
    }
    runtime.screen_state_mut().anvil_name = "Blade".into();
    let renamed = runtime.anvil_preview(&player).unwrap();
    assert!(!Arc::ptr_eq(&first, &renamed));
    assert_eq!(renamed.outcome.cost, first.outcome.cost + 1);
    correct_target(&mut runtime, &mut player, 100, "Server blade");
    runtime.sync_anvil_name(&player);
    let corrected = runtime.anvil_preview(&player).unwrap();
    assert!(!Arc::ptr_eq(&renamed, &corrected));
    assert_eq!(corrected.outcome.cost, 1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    let creative = runtime.anvil_preview(&player).unwrap();
    assert!(!Arc::ptr_eq(&corrected, &creative));
    runtime.set_server_lang(None);
    let localized = runtime.anvil_preview(&player).unwrap();
    assert!(!Arc::ptr_eq(&creative, &localized));
    let mut sword_entry = entry("minecraft:diamond_sword", SWORD);
    sword_entry.components = Some(Arc::new(protocol::ItemComponents {
        max_durability: Some(200),
        durability_declared: true,
        ..Default::default()
    }));
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&ItemRegistryEvent {
            entries: Arc::from([
                sword_entry,
                entry("minecraft:diamond", DIAMOND),
                entry("minecraft:enchanted_book", BOOK),
            ]),
        });
    let changed_registry = runtime.anvil_preview(&player).unwrap();
    assert!(!Arc::ptr_eq(&localized, &changed_registry));
    assert_eq!(changed_registry.outcome.material_used, 2);
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_TARGET_SLOT),
        NetworkItemStack::empty(),
    );
    assert!(
        runtime.anvil_preview(&player).is_none(),
        "a removed target clears the cached result"
    );
}

#[test]
fn changed_default_names_refresh_only_untouched_prefills() {
    let (mut runtime, player) = anvil();
    for (name, edited) in [("Original", false), ("Updated", false), ("Final", true)] {
        if edited {
            runtime.screen_state_mut().anvil_name = "Blade".into();
        }
        let input = format!("item.diamond_sword.name={name}\n");
        runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
            target.copy_from_slice(input.as_bytes());
            true
        }));
        runtime.sync_anvil_name(&player);
        assert_eq!(
            runtime.screen_state().anvil_name,
            if edited { "Blade" } else { name }
        );
        let preview = runtime.anvil_preview(&player).unwrap();
        assert_eq!(preview.outcome.cost, if edited { 4 } else { 3 });
        assert_eq!(
            preview.outcome.custom_name,
            edited.then(|| Some("Blade".into()))
        );
    }
}

#[test]
fn damage_only_nbt_updates_preserve_an_entered_rename() {
    let (mut runtime, mut player) = anvil();
    runtime.sync_anvil_name(&player);
    runtime.screen_state_mut().anvil_name = "Blade".into();
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            damage: Some(600),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, "Blade");
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.material_used, 2);
    assert_eq!(preview.outcome.cost, 3);
    assert_eq!(preview.outcome.custom_name, Some(Some("Blade".into())));
}

#[test]
fn captured_partial_repair_retains_result_durability() {
    use crate::ui_runtime::presentation::{
        UiPresentationRuntime,
        publish::{capture_hud_frame, item_icons::ItemIconFrames},
    };
    let (mut runtime, mut player) = anvil();
    let mut diamonds = runtime
        .inventory_ledger(&player)
        .target_stack(InventoryTarget::Craft(ANVIL_MATERIAL_SLOT))
        .unwrap()
        .clone();
    diamonds.count = 1;
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_MATERIAL_SLOT),
        diamonds,
    );
    runtime.inventory_open = true;
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
        entries: Arc::from([
            entry("minecraft:diamond_sword", SWORD),
            entry("minecraft:diamond", DIAMOND),
            entry("minecraft:enchanted_book", BOOK)
        ]),
    }));
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    capture_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        Some(&stream),
        semantic_input::PerspectiveMode::FirstPerson,
        0,
        ItemIconFrames::default(),
    );
    let output = presentation
        .hud_frame()
        .window_icons
        .recipe_output
        .as_ref()
        .unwrap();
    assert_eq!(protocol::item_stack_damage(&output.stack), Some(610));
    let maximum = assets::item::durability::builtin_item_durability("minecraft:diamond_sword")
        .maximum()
        .unwrap();
    assert_eq!(
        output.durability,
        Some((maximum - 610) as f32 / maximum as f32)
    );
}

#[test]
fn replacing_the_target_through_hotbar_swap_prefills_the_new_stack() {
    let (mut runtime, mut player) = anvil();
    let mut replacement = runtime
        .inventory_ledger(&player)
        .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
        .unwrap()
        .clone();
    replacement.stack_network_id = 32;
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 0,
            },
            stack: replacement,
            storage_item: None,
        }));
    runtime.sync_anvil_name(&player);
    let prefilled = runtime.screen_state().anvil_name.clone();
    runtime.screen_state_mut().anvil_name = "Blade".into();
    let request_id = runtime
        .inventory_ledger_mut(&mut player)
        .begin_hotbar_swap(InventoryTarget::Craft(ANVIL_TARGET_SLOT), 0)
        .unwrap();
    runtime.sync_anvil_name(&player);
    assert_eq!(
        runtime
            .inventory_ledger(&player)
            .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .unwrap()
            .stack_network_id,
        request_id
    );
    assert_eq!(runtime.screen_state().anvil_name, prefilled);
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, None);
    assert_eq!(preview.outcome.cost, 3);
}

#[test]
fn acknowledging_a_placement_preserves_an_entered_rename() {
    use inventory::inventory_ledger::CellGesture;
    use protocol::{
        ItemStackResponseEvent, StackResponse, StackResponseContainer, StackResponseSlot,
        StackResponseStatus,
    };
    let (mut runtime, mut player) = anvil();
    let target = InventoryTarget::Craft(ANVIL_TARGET_SLOT);
    runtime
        .inventory_ledger_mut(&mut player)
        .begin_target_gesture(target, CellGesture::Click)
        .unwrap();
    let request_id = runtime
        .inventory_ledger_mut(&mut player)
        .begin_target_gesture(target, CellGesture::Click)
        .unwrap();
    assert_eq!(
        runtime
            .inventory_ledger(&player)
            .target_stack(target)
            .unwrap()
            .stack_network_id,
        request_id
    );
    runtime.sync_anvil_name(&player);
    runtime.screen_state_mut().anvil_name = "Blade".into();
    let containers = [
        (
            protocol::ui_slot_container_name(ANVIL_TARGET_SLOT).unwrap(),
            ANVIL_TARGET_SLOT,
            true,
        ),
        (protocol::CONTAINER_NAME_CURSOR, 0, false),
    ]
    .map(|(slot_type, slot, occupied)| StackResponseContainer {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(slot_type),
            dynamic_id: None,
        },
        slots: Arc::from([StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count: u8::from(occupied),
            item_stack_id: if occupied { 30 } else { 0 },
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: if occupied { 1000 } else { 0 },
        }]),
    });
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                request_id,
                status: StackResponseStatus::Accepted,
                containers: Arc::from(containers),
            }]),
        }));
    assert_eq!(
        runtime
            .inventory_ledger(&player)
            .target_stack(target)
            .unwrap()
            .stack_network_id,
        30
    );
    assert!(
        runtime
            .inventory_ledger(&player)
            .target_retains_stack_id(target, request_id)
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, "Blade");
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, Some(Some("Blade".into())));
    assert_eq!(preview.outcome.cost, 4);

    let restated = runtime
        .inventory_ledger(&player)
        .target_stack(target)
        .unwrap()
        .clone();
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_TARGET_SLOT),
        restated,
    );
    assert!(
        !runtime
            .inventory_ledger(&player)
            .target_retains_stack_id(target, request_id)
    );
}

#[test]
fn drained_authority_updates_anvil_names_before_a_same_frame_take() {
    let (mut runtime, mut player) = anvil();
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    runtime.sync_anvil_name(&player);
    let old_name = runtime.screen_state().anvil_name.clone();
    let mut corrected = runtime
        .inventory_ledger(&player)
        .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
        .unwrap()
        .clone();
    corrected.extra_data = protocol::edit_item_extra(
        &corrected.extra_data,
        &protocol::ItemNbtEdit {
            custom_name: Some(Some("Server blade".into())),
            ..Default::default()
        },
    )
    .unwrap();
    corrected.nbt_digest = Sha256::digest(&corrected.extra_data).into();
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity {
                        window_id: Some(124),
                        slot_type: protocol::ui_slot_container_name(ANVIL_TARGET_SLOT),
                        dynamic_id: None,
                    },
                    slot: u16::from(ANVIL_TARGET_SLOT),
                },
                stack: corrected,
                storage_item: None,
            }),
        )
        .unwrap();
    assert_eq!(
        runtime.screen_state().anvil_name,
        old_name,
        "ingress waits for the authority drain"
    );
    runtime.drain_pending_inventory(&mut player);
    assert_eq!(runtime.screen_state().anvil_name, "Server blade");
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, None);
    assert_eq!(
        preview.outcome.cost, 3,
        "only the three repair materials cost levels"
    );
    runtime.perform_pointer_action(
        &mut player,
        crate::ui_runtime::inventory_drag::PointerAction::Click(
            ui_screens::container_actions::InventoryCellHit::CraftOutput,
        ),
    );
    let ledger = runtime.inventory_ledger(&player);
    assert_eq!(ledger.pending_request_count(), 1);
    assert_eq!(
        protocol::item_custom_name(&ledger.cursor_stack().unwrap().extra_data).as_deref(),
        Some("Server blade"),
        "the actual take must retain the corrected name"
    );
}

#[test]
fn count_only_predictions_preserve_a_rename_on_the_remaining_target() {
    use inventory::inventory_ledger::{CellGesture, DropSource};
    for split in [true, false] {
        let (mut runtime, mut player) = anvil();
        let target = InventoryTarget::Craft(ANVIL_TARGET_SLOT);
        let stack = NetworkItemStack {
            network_id: DIAMOND,
            stack_network_id: 30,
            count: 4,
            ..NetworkItemStack::empty()
        };
        place(
            &mut runtime,
            &mut player,
            u16::from(ANVIL_TARGET_SLOT),
            stack,
        );
        runtime.sync_anvil_name(&player);
        runtime.screen_state_mut().anvil_name = "Blade".into();
        let ledger = runtime.inventory_ledger_mut(&mut player);
        if split {
            ledger
                .begin_target_gesture(target, CellGesture::TakeCount(2))
                .unwrap();
        } else {
            ledger
                .begin_drop(DropSource::Target(target), Some(1))
                .unwrap();
        }
        runtime.sync_anvil_name(&player);
        assert_eq!(runtime.screen_state().anvil_name, "Blade");
        assert_eq!(
            runtime
                .inventory_ledger(&player)
                .target_stack(target)
                .unwrap()
                .count,
            if split { 2 } else { 3 }
        );
    }
}

#[test]
fn enchanting_a_plain_book_predicts_the_negotiated_enchanted_book_kind() {
    for count in [1, 64] {
        let (mut runtime, mut player) = anvil();
        let plain_book = 13;
        let mut entries: Vec<_> = runtime
            .inventory_ledger(&player)
            .negotiated_item_registry()
            .unwrap()
            .values()
            .cloned()
            .collect();
        entries.push(entry("minecraft:book", plain_book));
        runtime
            .inventory_ledger_mut(&mut player)
            .apply_registry(&ItemRegistryEvent {
                entries: entries.into(),
            });
        place(
            &mut runtime,
            &mut player,
            u16::from(ANVIL_TARGET_SLOT),
            NetworkItemStack {
                network_id: plain_book,
                stack_network_id: 34,
                count,
                ..NetworkItemStack::empty()
            },
        );
        place_book(&mut runtime, &mut player, vec![(9, 1)]);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Creative);
        runtime.sync_anvil_name(&player);
        let preview = runtime.anvil_preview(&player).unwrap();
        assert_eq!(preview.stack.network_id, BOOK);
        assert_eq!(preview.stack.count, 1);
        assert!(preview.outcome.enchant_book);
        assert_eq!(
            protocol::item_anvil_data(&preview.stack.extra_data)
                .unwrap()
                .enchantments,
            [(9, 1)]
        );
        let input = |slot| {
            let ledger = runtime.inventory_ledger(&player);
            protocol::StackRequestSlot {
                container: protocol::ui_slot_request_container(slot).unwrap(),
                slot,
                stack_network_id: ledger
                    .target_stack(InventoryTarget::Craft(slot))
                    .unwrap()
                    .stack_network_id,
            }
        };
        let target = input(ANVIL_TARGET_SLOT);
        let material = input(ANVIL_MATERIAL_SLOT);
        runtime.perform_pointer_action(
            &mut player,
            crate::ui_runtime::inventory_drag::PointerAction::Click(
                ui_screens::container_actions::InventoryCellHit::CraftOutput,
            ),
        );
        let ledger = runtime.inventory_ledger(&player);
        assert_eq!(ledger.cursor_stack().unwrap().network_id, BOOK);
        assert_eq!(ledger.cursor_stack().unwrap().count, 1);
        assert_eq!(ledger.pending_request_count(), 1);
        use protocol::{
            StackRequestAction as Action, StackRequestContainer as Container, StackRequestSlot,
        };
        let (packet, requests) = ledger.pending_batch().unwrap().unwrap();
        assert_eq!(requests, 1);
        let request_id = ledger.pending_request_id().unwrap();
        // The prefilled book name is not the enchanted book's default name, so it is sent.
        let typed = runtime.screen_state().anvil_name.clone();
        let expected = protocol::item_stack_request_packet_filtered(
            request_id,
            &[
                Action::CraftRecipeOptional {
                    recipe_network_id: 0,
                    filtered_string_index: 0,
                },
                Action::Consume {
                    amount: 1,
                    source: material,
                },
                Action::Consume {
                    amount: u8::try_from(count).unwrap(),
                    source: target,
                },
                Action::Take {
                    amount: 1,
                    source: StackRequestSlot {
                        container: Container::CreatedOutput,
                        slot: protocol::CREATED_OUTPUT_SLOT,
                        stack_network_id: request_id,
                    },
                    destination: StackRequestSlot {
                        container: Container::Cursor,
                        slot: 0,
                        stack_network_id: 0,
                    },
                },
            ],
            &[typed],
        )
        .unwrap();
        assert_eq!(packet, expected, "the take carries a single book");
    }
}

#[test]
fn too_expensive_predictions_keep_costs_but_publish_no_result_or_hover() {
    use crate::ui_runtime::presentation::{
        UiPresentationRuntime,
        publish::{capture_hud_frame, hovered_stack, item_icons::ItemIconFrames},
    };
    let (mut runtime, mut player) = anvil();
    runtime.inventory_open = true;
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    runtime.hud.set_experience(0, 0.0);
    runtime.sync_anvil_name(&player);
    let low_cost = runtime.anvil_preview(&player).unwrap();
    assert!(low_cost.outcome.cost < inventory::ANVIL_TOO_EXPENSIVE);
    assert!(!runtime.anvil_affordable(&player, &low_cost));
    assert!(
        runtime.anvil_output(&player).is_some(),
        "insufficient levels still show the result"
    );
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            repair_cost: Some(inventory::ANVIL_TOO_EXPENSIVE - 3),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    runtime.screen_state_mut().hover =
        Some(ui_screens::container_actions::InventoryCellHit::CraftOutput);
    let expensive = runtime.anvil_preview(&player).unwrap();
    assert_eq!(expensive.outcome.cost, inventory::ANVIL_TOO_EXPENSIVE);
    assert!(expensive.outcome.too_expensive);
    assert!(runtime.anvil_output(&player).is_none());
    assert!(hovered_stack(&player, &runtime).is_none());
    runtime.perform_pointer_action(
        &mut player,
        crate::ui_runtime::inventory_drag::PointerAction::Click(
            ui_screens::container_actions::InventoryCellHit::CraftOutput,
        ),
    );
    assert_eq!(runtime.inventory_ledger(&player).pending_request_count(), 0);
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(ItemRegistryEvent {
        entries: Arc::from([
            entry("minecraft:diamond_sword", SWORD),
            entry("minecraft:diamond", DIAMOND),
            entry("minecraft:enchanted_book", BOOK),
        ])
    }));
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    capture_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        Some(&stream),
        semantic_input::PerspectiveMode::FirstPerson,
        0,
        ItemIconFrames::default(),
    );
    assert!(
        presentation
            .hud_frame()
            .window_icons
            .recipe_output
            .is_none()
    );

    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    let creative = runtime.anvil_preview(&player).unwrap();
    assert!(!creative.outcome.too_expensive);
    assert!(runtime.anvil_output(&player).is_some());
    assert!(hovered_stack(&player, &runtime).is_some());

    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let mut authoritative = expensive.stack.clone();
    authoritative.stack_network_id = 100;
    place(
        &mut runtime,
        &mut player,
        u16::from(protocol::CREATED_OUTPUT_SLOT),
        authoritative.clone(),
    );
    assert_eq!(runtime.anvil_output(&player), Some(authoritative.clone()));
    assert_eq!(
        hovered_stack(&player, &runtime),
        Some((authoritative, None))
    );
}

/// Opens an anvil with a registered component weapon and a sharpness book.
fn component_weapon_anvil() -> (UiRuntime, player_state::PlayerState) {
    let (mut runtime, mut player) = anvil();
    let mut weapon = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:stick")
        .unwrap()
        .clone();
    weapon.network_id = SWORD;
    weapon.component_based = true;
    weapon.version = ItemRegistryVersion::DataDriven;
    weapon.item_tags = Arc::from([Arc::from("minecraft:is_sword")]);
    weapon.components = Some(Arc::new(protocol::ItemComponents {
        durability_declared: true,
        max_durability: Some(200),
        ..Default::default()
    }));
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&ItemRegistryEvent {
            entries: Arc::from([
                weapon,
                entry("minecraft:diamond", DIAMOND),
                entry("minecraft:enchanted_book", BOOK),
            ]),
        });
    place_book(&mut runtime, &mut player, vec![(9, 1)]);
    (runtime, player)
}

#[test]
fn component_weapon_preview_uses_its_declared_enchantment_class() {
    let (mut runtime, player) = component_weapon_anvil();
    runtime.sync_anvil_name(&player);
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.stack.network_id, SWORD);
    assert_eq!(preview.outcome.enchantments, Some(vec![(9, 1)]));
    assert_eq!(preview.outcome.cost, 1);
}

#[test]
fn snapshot_component_weapon_anvil() {
    use crate::ui_runtime::presentation::{UiPresentationRuntime, forms::snapshot};
    let Some(directory) = std::env::var_os("CINNABAR_ANVIL_CAPTURE_CARRIER_DIR") else {
        eprintln!(
            "skipping snapshot_component_weapon_anvil: missing CINNABAR_ANVIL_CAPTURE_CARRIER_DIR"
        );
        return;
    };
    let Some(output) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") else {
        eprintln!("skipping snapshot_component_weapon_anvil: missing CINNABAR_FORM_SNAPSHOT_DIR");
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    for carrier in [
        assets::carriers::FONT,
        assets::carriers::HUD,
        assets::carriers::UI,
        assets::carriers::LANG,
    ] {
        if !directory.join(carrier.output).is_file() {
            eprintln!(
                "skipping snapshot_component_weapon_anvil: missing {} fixture",
                carrier.output
            );
            return;
        }
    }
    let read =
        |carrier: assets::carriers::Carrier| std::fs::read(directory.join(carrier.output)).unwrap();
    let manifest = assets::canonical_source_manifest_sha256(
        assets::carriers::FONT.font_face.unwrap().manifest,
    );
    let font = Arc::new(
        assets::RuntimeFontCatalog::decode(&read(assets::carriers::FONT), manifest)
            .unwrap()
            .with_coverage_pages(),
    );
    let hud = Arc::new(assets::RuntimeHudCatalog::decode(&read(assets::carriers::HUD)).unwrap());
    let ui = Arc::new(assets::RuntimeUiAssets::decode(&read(assets::carriers::UI)).unwrap());
    let lang = Arc::new(assets::RuntimeLangCatalog::decode(&read(assets::carriers::LANG)).unwrap());
    let mut presentation = UiPresentationRuntime::with_hud(font, hud).unwrap();
    presentation.enable_json_ui(ui).unwrap();
    presentation.set_gui_scale_preference(Some(2));
    let (mut runtime, mut player) = component_weapon_anvil();
    runtime.inventory_open = true;
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    runtime.set_lang_catalog(lang);
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            damage: Some(100),
            custom_name: Some(Some("Tagged weapon".to_owned())),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    let dpi = ui::DpiScale::new(1.0).unwrap();
    for now in [0, 500] {
        presentation
            .build(&player, &runtime, now, [1280, 720], dpi)
            .unwrap();
    }
    let input = presentation
        .build(&player, &runtime, 5_000, [1280, 720], dpi)
        .unwrap();
    assert!(!input.indices.is_empty());
    std::fs::create_dir_all(output).unwrap();
    snapshot::write(&input, "anvil-component-weapon");
}
