use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent, InventorySlotEvent,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, SlotIdentity,
};

use super::*;

const SWORD: i32 = 10;
const DIAMOND: i32 = 11;
const BOOK: i32 = 12;

/// A vanilla registry entry for `identifier`.
fn entry(identifier: &str, network_id: i32) -> ItemRegistryEntry {
    ItemRegistryEntry {
        identifier: Arc::from(identifier),
        network_id,
        component_based: false,
        version: ItemRegistryVersion::None,
        components: None,
        component_digest: [0; 32],
        negotiated_max_stack_size: None,
        canonical_empty_component_data: true,
        item_tags: Arc::from([]),
    }
}

/// Puts `stack` in anvil UI `slot`.
fn place(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    slot: u16,
    stack: NetworkItemStack,
) {
    runtime
        .inventory_ledger_mut(player)
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(124),
                    slot_type: protocol::ui_slot_container_name(u8::try_from(slot).unwrap()),
                    dynamic_id: None,
                },
                slot,
            },
            stack,
            storage_item: None,
        }));
}

/// Rewrites the occupied anvil input's user data and updates its retained digest.
fn edit_input(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    slot: u8,
    edit: protocol::ItemNbtEdit,
) {
    let mut stack = runtime
        .inventory_ledger(player)
        .target_stack(InventoryTarget::Craft(slot))
        .unwrap()
        .clone();
    stack.extra_data = protocol::edit_item_extra(&stack.extra_data, &edit).unwrap();
    stack.nbt_digest = Sha256::digest(&stack.extra_data).into();
    place(runtime, player, u16::from(slot), stack);
}

/// Replaces the second input with a book carrying the complete enchantment list.
fn place_book(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    enchantments: Vec<(i16, i16)>,
) {
    let extra_data = protocol::edit_item_extra(
        &[],
        &protocol::ItemNbtEdit {
            enchantments: Some(enchantments),
            ..Default::default()
        },
    )
    .unwrap();
    place(
        runtime,
        player,
        u16::from(ANVIL_MATERIAL_SLOT),
        NetworkItemStack {
            network_id: BOOK,
            stack_network_id: 32,
            count: 1,
            nbt_digest: Sha256::digest(&extra_data).into(),
            extra_data,
            ..NetworkItemStack::empty()
        },
    );
}

/// An open anvil holding a sword with 1000 damage and ten diamonds.
fn anvil() -> (UiRuntime, player_state::PlayerState) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([
            entry("minecraft:diamond_sword", SWORD),
            entry("minecraft:diamond", DIAMOND),
            entry("minecraft:enchanted_book", BOOK),
        ]),
    });
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(5),
        window_type: protocol::WINDOW_TYPE_ANVIL,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    let damaged = protocol::edit_item_extra(
        &[],
        &protocol::ItemNbtEdit {
            damage: Some(1000),
            ..protocol::ItemNbtEdit::default()
        },
    )
    .unwrap();
    let sword = NetworkItemStack {
        network_id: SWORD,
        stack_network_id: 30,
        count: 1,
        nbt_digest: Sha256::digest(&damaged).into(),
        extra_data: damaged,
        ..NetworkItemStack::empty()
    };
    place(&mut runtime, &mut player, 1, sword);
    let diamonds = NetworkItemStack {
        network_id: DIAMOND,
        stack_network_id: 31,
        count: 10,
        ..NetworkItemStack::empty()
    };
    place(&mut runtime, &mut player, 2, diamonds);
    (runtime, player)
}

#[test]
fn repairs_preview_the_mended_result_and_its_cost() {
    let (mut runtime, player) = anvil();
    runtime.sync_anvil_name(&player);
    let preview = runtime
        .anvil_preview(&player)
        .expect("diamonds repair the sword");
    assert_eq!(protocol::item_stack_damage(&preview.stack), Some(0));
    assert_eq!(protocol::item_repair_cost(&preview.stack.extra_data), 1);
    assert_eq!(preview.outcome.material_used, 3);
    assert_eq!(preview.outcome.cost, 3);
    assert_eq!(
        preview.outcome.custom_name, None,
        "the prefilled name renames nothing"
    );
    assert!(
        !runtime.anvil_affordable(&player, &preview),
        "no levels yet"
    );
}

#[test]
fn unrelated_components_keep_builtin_durability_for_materials_and_copies() {
    for combine_copy in [false, true] {
        let (mut runtime, mut player) = anvil();
        let mut sword = entry("minecraft:diamond_sword", SWORD);
        sword.components = Some(Arc::new(protocol::ItemComponents {
            display_name: Some("Sword".into()),
            ..Default::default()
        }));
        runtime
            .inventory_ledger_mut(&mut player)
            .apply_registry(&ItemRegistryEvent {
                entries: Arc::from([sword, entry("minecraft:diamond", DIAMOND)]),
            });
        if combine_copy {
            let mut copy = runtime
                .inventory_ledger(&player)
                .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
                .unwrap()
                .clone();
            copy.stack_network_id = 31;
            place(
                &mut runtime,
                &mut player,
                u16::from(ANVIL_MATERIAL_SLOT),
                copy,
            );
        }
        runtime.sync_anvil_name(&player);
        let preview = runtime
            .anvil_preview(&player)
            .expect("unrelated components do not remove durability");
        assert_eq!(
            protocol::item_stack_damage(&preview.stack),
            Some(if combine_copy { 252 } else { 0 })
        );
        assert_eq!(preview.outcome.cost, if combine_copy { 2 } else { 3 });
    }
}

#[test]
fn unsupported_durability_declarations_do_not_enable_repair_predictions() {
    for maximum in [None, Some(0)] {
        let (mut runtime, mut player) = anvil();
        let mut sword = entry("minecraft:diamond_sword", SWORD);
        sword.components = Some(Arc::new(protocol::ItemComponents {
            durability_declared: true,
            max_durability: maximum,
            ..Default::default()
        }));
        runtime
            .inventory_ledger_mut(&mut player)
            .apply_registry(&ItemRegistryEvent {
                entries: Arc::from([sword, entry("minecraft:diamond", DIAMOND)]),
            });
        runtime.sync_anvil_name(&player);
        assert!(runtime.anvil_preview(&player).is_none());
    }
}

#[test]
fn the_name_box_prefills_from_the_target_and_renames_cost_a_level() {
    let (mut runtime, mut player) = anvil();
    runtime.sync_anvil_name(&player);
    let prefilled = runtime.screen_state().anvil_name.clone();
    assert!(!prefilled.is_empty());
    runtime.screen_state_mut().anvil_name = "Blade".into();
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, "Blade", "typing is kept");
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, Some(Some("Blade".into())));
    assert_eq!(preview.outcome.cost, 4);
    assert_eq!(
        protocol::item_display(&preview.stack.extra_data)
            .name
            .as_deref(),
        Some("Blade")
    );
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    assert!(runtime.anvil_affordable(&player, &preview));
}

/// The sword and a Sharpness I book: taking the merge without renaming sends the take.
#[test]
fn taking_a_book_merge_without_renaming_sends_the_take() {
    for rename in [None, Some("Blade")] {
        let (mut runtime, mut player) = anvil();
        let ench = protocol::edit_item_extra(
            &[],
            &protocol::ItemNbtEdit {
                enchantments: Some(vec![(9, 1)]),
                ..protocol::ItemNbtEdit::default()
            },
        )
        .unwrap();
        let book = NetworkItemStack {
            network_id: BOOK,
            stack_network_id: 32,
            count: 1,
            nbt_digest: Sha256::digest(&ench).into(),
            extra_data: ench,
            ..NetworkItemStack::empty()
        };
        place(&mut runtime, &mut player, 2, book);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Creative);
        runtime.sync_anvil_name(&player);
        if let Some(name) = rename {
            runtime.screen_state_mut().anvil_name = name.into();
        }
        assert!(
            runtime.anvil_preview(&player).is_some(),
            "{rename:?}: a result shows"
        );
        runtime.perform_pointer_action(
            &mut player,
            crate::ui_runtime::inventory_drag::PointerAction::Click(
                ui_screens::container_actions::InventoryCellHit::CraftOutput,
            ),
        );
        let ledger = runtime.inventory_ledger(&player);
        assert_eq!(
            ledger.pending_request_count(),
            1,
            "{rename:?}: the take is queued"
        );
        assert!(
            matches!(ledger.pending_batch(), Ok(Some(_))),
            "{rename:?}: the take encodes: {:?}",
            ledger.pending_batch().err()
        );
    }
}

#[test]
fn server_outputs_are_taken_even_when_prediction_differs_or_is_unavailable() {
    use protocol::{
        CraftResult, StackRequestAction as Action, StackRequestContainer as Container,
        StackRequestSlot,
    };
    for predictable in [false, true] {
        let (mut runtime, mut player) = anvil();
        if !predictable {
            edit_input(
                &mut runtime,
                &mut player,
                ANVIL_TARGET_SLOT,
                protocol::ItemNbtEdit {
                    damage: Some(0),
                    ..Default::default()
                },
            );
        }
        runtime.sync_anvil_name(&player);
        assert_eq!(runtime.anvil_preview(&player).is_some(), predictable);
        let extra_data = protocol::edit_item_extra(
            &[],
            &protocol::ItemNbtEdit {
                custom_name: Some(Some("Server result".into())),
                ..Default::default()
            },
        )
        .unwrap();
        let output = NetworkItemStack {
            network_id: DIAMOND,
            count: 2,
            stack_network_id: 70,
            nbt_digest: Sha256::digest(&extra_data).into(),
            extra_data,
            ..NetworkItemStack::empty()
        };
        place(
            &mut runtime,
            &mut player,
            u16::from(protocol::CREATED_OUTPUT_SLOT),
            output.clone(),
        );
        runtime.screen_state_mut().hover =
            Some(ui_screens::container_actions::InventoryCellHit::CraftOutput);
        assert_eq!(runtime.anvil_output(&player), Some(output.clone()));
        assert_eq!(
            crate::ui_runtime::presentation::publish::hovered_stack(&player, &runtime),
            Some((output.clone(), None))
        );
        let name = runtime.screen_state().anvil_name.trim().to_owned();
        runtime.perform_pointer_action(
            &mut player,
            crate::ui_runtime::inventory_drag::PointerAction::Click(
                ui_screens::container_actions::InventoryCellHit::CraftOutput,
            ),
        );
        let ledger = runtime.inventory_ledger(&player);
        let cursor = ledger
            .cursor_stack()
            .expect("a server result can be taken without a local cost");
        assert_eq!(
            (cursor.network_id, cursor.count),
            (output.network_id, output.count)
        );
        assert_eq!(cursor.extra_data, output.extra_data);
        let (actual, count) = ledger.pending_batch().unwrap().unwrap();
        assert_eq!(count, 1);
        let expected = protocol::item_stack_request_packet_filtered(
            -3,
            &[
                Action::CraftRecipeOptional {
                    recipe_network_id: 0,
                    filtered_string_index: 0,
                },
                Action::CraftResultsDeprecated {
                    results: Arc::from([CraftResult {
                        identifier: Arc::from("minecraft:diamond"),
                        aux: 0,
                        count: output.count,
                        block_runtime_id: 0,
                        user_data: output.extra_data,
                    }]),
                    crafts: 1,
                },
                Action::Consume {
                    amount: 1,
                    source: StackRequestSlot {
                        container: protocol::ui_slot_request_container(ANVIL_TARGET_SLOT).unwrap(),
                        slot: ANVIL_TARGET_SLOT,
                        stack_network_id: 30,
                    },
                },
                Action::Consume {
                    amount: 1,
                    source: StackRequestSlot {
                        container: protocol::ui_slot_request_container(ANVIL_MATERIAL_SLOT)
                            .unwrap(),
                        slot: ANVIL_MATERIAL_SLOT,
                        stack_network_id: 31,
                    },
                },
                Action::Take {
                    amount: 2,
                    source: StackRequestSlot {
                        container: Container::CreatedOutput,
                        slot: protocol::CREATED_OUTPUT_SLOT,
                        stack_network_id: 70,
                    },
                    destination: StackRequestSlot {
                        container: Container::Cursor,
                        slot: 0,
                        stack_network_id: 0,
                    },
                },
            ],
            &[name],
        )
        .unwrap();
        assert_eq!(
            actual, expected,
            "the request submits the displayed server stack"
        );
        assert_eq!(
            ledger
                .target_stack(InventoryTarget::Craft(ANVIL_MATERIAL_SLOT))
                .unwrap()
                .count,
            9
        );
    }
}

#[test]
fn predicted_output_hover_includes_the_merged_enchantments_name_and_lore() {
    let (mut runtime, mut player) = anvil();
    let mut target = runtime
        .inventory_ledger(&player)
        .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
        .unwrap()
        .clone();
    let mut extra = vec![0xff, 0xff, 1, 10, 0, 0, 10, 7, 0];
    extra.extend_from_slice(b"display");
    extra.extend_from_slice(&[9, 4, 0]);
    extra.extend_from_slice(b"Lore");
    extra.push(8);
    extra.extend_from_slice(&1i32.to_le_bytes());
    extra.extend_from_slice(&4u16.to_le_bytes());
    extra.extend_from_slice(b"Lore");
    extra.extend_from_slice(&[0, 0]);
    extra.extend_from_slice(&[0; 8]);
    target.extra_data = extra.into();
    target.nbt_digest = Sha256::digest(&target.extra_data).into();
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_TARGET_SLOT),
        target,
    );
    place_book(&mut runtime, &mut player, vec![(9, 1)]);
    runtime.sync_anvil_name(&player);
    runtime.screen_state_mut().anvil_name = "Blade".into();
    runtime.screen_state_mut().hover =
        Some(ui_screens::container_actions::InventoryCellHit::CraftOutput);
    let shown = runtime.anvil_output(&player).unwrap();
    let (hovered, name) =
        crate::ui_runtime::presentation::publish::hovered_stack(&player, &runtime)
            .expect("a predicted output has hover data");
    assert_eq!(hovered, shown);
    assert_eq!(name, None);
    let display = protocol::item_display(&hovered.extra_data);
    assert_eq!(display.name.as_deref(), Some("Blade"));
    assert_eq!(display.enchantments, vec![(9, 1)]);
    assert_eq!(display.lore, vec![Arc::<str>::from("Lore")]);
}

#[test]
fn merging_preserves_every_existing_enchantment_and_full_levels() {
    let (mut runtime, mut player) = anvil();
    let mut enchantments = vec![(9, 300)];
    enchantments.extend((100..140).map(|id| (id, 1)));
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            enchantments: Some(enchantments.clone()),
            ..Default::default()
        },
    );
    place_book(&mut runtime, &mut player, vec![(17, 1)]);
    runtime.sync_anvil_name(&player);
    let preview = runtime.anvil_preview(&player).unwrap();
    enchantments.push((17, 1));
    assert_eq!(
        protocol::item_anvil_data(&preview.stack.extra_data)
            .unwrap()
            .enchantments,
        enchantments
    );
    assert_eq!(preview.outcome.cost, 1);
}

#[test]
fn unsupported_enchantment_fields_on_either_input_decline_prediction() {
    for slot in [ANVIL_TARGET_SLOT, ANVIL_MATERIAL_SLOT] {
        let (mut runtime, mut player) = anvil();
        edit_input(
            &mut runtime,
            &mut player,
            ANVIL_TARGET_SLOT,
            protocol::ItemNbtEdit {
                enchantments: Some(vec![(9, 1)]),
                ..Default::default()
            },
        );
        place_book(&mut runtime, &mut player, vec![(17, 1)]);
        let mut input = runtime
            .inventory_ledger(&player)
            .target_stack(InventoryTarget::Craft(slot))
            .unwrap()
            .clone();
        let mut extra = input.extra_data.to_vec();
        let end = extra.len() - 10;
        let mut field = vec![2, 6, 0];
        field.extend_from_slice(b"custom");
        field.extend_from_slice(&42i16.to_le_bytes());
        extra.splice(end..end, field);
        input.extra_data = extra.into();
        input.nbt_digest = Sha256::digest(&input.extra_data).into();
        assert_eq!(protocol::item_anvil_data(&input.extra_data), None);
        place(&mut runtime, &mut player, u16::from(slot), input);
        runtime.sync_anvil_name(&player);
        runtime.screen_state_mut().anvil_name = "Blade".into();
        assert!(
            runtime.anvil_preview(&player).is_none(),
            "unsupported input {slot} cannot be ignored"
        );
    }
}

#[test]
fn full_custom_names_are_kept_when_repairing_without_a_rename() {
    let (mut runtime, mut player) = anvil();
    let name = "Blade".repeat(300);
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            custom_name: Some(Some(name.clone())),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, name);
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, None);
    assert_eq!(
        protocol::item_anvil_data(&preview.stack.extra_data)
            .unwrap()
            .custom_name
            .as_deref(),
        Some(name.as_str())
    );
}

/// Accepts a take and place so a server correction travels back into the target cell.
fn correct_target(
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
fn response_corrections_drive_repair_cost_and_name_without_replacing_nbt() {
    for (damage, material_used) in [(-1, 3), (0, 0), (100, 1), (800, 3)] {
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
            protocol::item_stack_damage(target),
            Some(1000),
            "NBT remains stale"
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
            assets::POTION_DESCRIPTORS
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
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    capture_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        None,
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
