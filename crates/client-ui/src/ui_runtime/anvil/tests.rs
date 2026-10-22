use std::sync::Arc;

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent, InventorySlotEvent,
    ItemRegistryEntry, ItemRegistryEvent, ItemRegistryVersion, SlotIdentity,
};

use super::*;

mod name_acknowledgements;

const SWORD: i32 = 10;
const DIAMOND: i32 = 11;
const BOOK: i32 = 12;

mod presentation;

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

/// The name a take sends: the prediction's, else the target's take name.
fn take_rename(runtime: &UiRuntime, player: &player_state::PlayerState) -> Option<Arc<str>> {
    if let Some(preview) = runtime.anvil_preview(player) {
        return preview.take_name.clone();
    }
    let target = runtime
        .inventory_ledger(player)
        .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))?
        .clone();
    runtime.anvil_take_name(player, target.network_id, target.metadata)
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

/// Corrects a worn inventory item, then moves it into the anvil without restating its NBT.
fn correct_input(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    slot: u8,
    damage: i32,
    name: &str,
) {
    use inventory::inventory_ledger::CellGesture;
    let stack = runtime
        .inventory_ledger(player)
        .target_stack(InventoryTarget::Craft(slot))
        .unwrap()
        .clone();
    place(runtime, player, u16::from(slot), NetworkItemStack::empty());
    let ledger = runtime.inventory_ledger_mut(player);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 0,
        },
        stack,
        storage_item: None,
    }));
    let request_id = ledger.begin_mining_request(0, damage).unwrap();
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                request_id,
                status: protocol::StackResponseStatus::Accepted,
                containers: Arc::from([protocol::StackResponseContainer {
                    container: ContainerIdentity {
                        window_id: None,
                        slot_type: Some(protocol::CONTAINER_NAME_HOTBAR),
                        dynamic_id: None,
                    },
                    slots: Arc::from([protocol::StackResponseSlot {
                        slot: 0,
                        hotbar_slot: 0,
                        count: 1,
                        item_stack_id: 75,
                        custom_name: Arc::from(name),
                        filtered_custom_name: Arc::from(""),
                        durability_correction: damage,
                    }]),
                }]),
            }]),
        },
    ));
    ledger
        .begin_target_gesture(InventoryTarget::Player(0), CellGesture::Click)
        .unwrap();
    ledger
        .begin_target_gesture(InventoryTarget::Craft(slot), CellGesture::Click)
        .unwrap();
    assert_eq!(
        ledger
            .presented_target_overlay(InventoryTarget::Craft(slot))
            .unwrap()
            .durability_correction,
        Some(damage)
    );
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
fn repair_predictions_use_materialized_server_damage() {
    for corrected_damage in [0, 1000] {
        let (mut runtime, mut player) = anvil();
        let raw_damage = if corrected_damage == 0 { 1000 } else { 0 };
        edit_input(
            &mut runtime,
            &mut player,
            ANVIL_TARGET_SLOT,
            protocol::ItemNbtEdit {
                damage: Some(raw_damage),
                ..Default::default()
            },
        );
        correct_input(
            &mut runtime,
            &mut player,
            ANVIL_TARGET_SLOT,
            corrected_damage,
            "",
        );
        let raw = runtime
            .inventory_ledger(&player)
            .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .unwrap();
        assert_eq!(
            protocol::item_stack_damage(raw),
            Some(corrected_damage as u32)
        );
        runtime.sync_anvil_name(&player);
        let preview = runtime.anvil_preview(&player);
        if corrected_damage == 0 {
            assert!(
                preview.is_none(),
                "a corrected pristine item needs no repair"
            );
        } else {
            let preview = preview.expect("a worn item with pristine NBT can be repaired");
            assert_eq!(preview.outcome.material_used, 3);
            assert_eq!(preview.outcome.cost, 3);
            assert_eq!(protocol::item_stack_damage(&preview.stack), Some(0));
        }
    }
}

#[test]
fn combining_copies_uses_the_second_inputs_corrected_damage() {
    let (mut runtime, mut player) = anvil();
    let mut copy = runtime
        .inventory_ledger(&player)
        .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
        .unwrap()
        .clone();
    copy.stack_network_id = 31;
    copy.extra_data = protocol::edit_item_extra(
        &copy.extra_data,
        &protocol::ItemNbtEdit {
            damage: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    copy.nbt_digest = Sha256::digest(&copy.extra_data).into();
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_MATERIAL_SLOT),
        copy,
    );
    correct_input(&mut runtime, &mut player, ANVIL_MATERIAL_SLOT, 1000, "");
    runtime.sync_anvil_name(&player);
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(protocol::item_stack_damage(&preview.stack), Some(252));
    assert_eq!(preview.outcome.cost, 2);
}

#[test]
fn unchanged_response_names_and_damage_survive_a_predicted_rename() {
    let (mut runtime, mut player) = anvil();
    correct_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        800,
        " Server Blade ",
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, " Server Blade ");
    let repaired = runtime.anvil_preview(&player).unwrap();
    assert_eq!(repaired.outcome.custom_name, None);
    assert_eq!(repaired.outcome.cost, 3);
    assert_eq!(
        protocol::item_custom_name(&repaired.stack.extra_data).as_deref(),
        Some(" Server Blade ")
    );
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_MATERIAL_SLOT),
        NetworkItemStack::empty(),
    );
    runtime.screen_state_mut().anvil_name = "New".into();
    let renamed = runtime.anvil_preview(&player).unwrap();
    assert_eq!(renamed.outcome.damage, None);
    assert_eq!(renamed.outcome.cost, 1);
    assert_eq!(protocol::item_stack_damage(&renamed.stack), Some(800));
    assert_eq!(
        protocol::item_custom_name(&renamed.stack.extra_data).as_deref(),
        Some("New")
    );
}

#[test]
fn damage_corrections_do_not_overwrite_a_name_the_player_is_editing() {
    for original_name in [None, Some(" Original ")] {
        for damage in [0, 800] {
            let (mut runtime, mut player) = anvil();
            if let Some(name) = original_name {
                edit_input(
                    &mut runtime,
                    &mut player,
                    ANVIL_TARGET_SLOT,
                    protocol::ItemNbtEdit {
                        custom_name: Some(Some(name.into())),
                        ..Default::default()
                    },
                );
            }
            runtime.sync_anvil_name(&player);
            runtime.screen_state_mut().anvil_name = "Typed".into();
            correct_input(&mut runtime, &mut player, ANVIL_TARGET_SLOT, damage, "");
            for _ in 0..2 {
                runtime.sync_anvil_name(&player);
                assert_eq!(runtime.screen_state().anvil_name, "Typed");
            }
            let with_material = runtime.anvil_preview(&player);
            if damage == 0 {
                assert!(
                    with_material.is_none(),
                    "the repair material changes nothing"
                );
            } else {
                let repaired = with_material.unwrap();
                assert_eq!(repaired.outcome.custom_name, Some(Some("Typed".into())));
                assert_eq!(protocol::item_stack_damage(&repaired.stack), Some(0));
                assert_eq!(repaired.outcome.cost, 4);
            }
            place(
                &mut runtime,
                &mut player,
                u16::from(ANVIL_MATERIAL_SLOT),
                NetworkItemStack::empty(),
            );
            runtime.sync_anvil_name(&player);
            assert_eq!(runtime.screen_state().anvil_name, "Typed");
            let renamed = runtime
                .anvil_preview(&player)
                .expect("the corrected target alone can be renamed");
            assert_eq!(renamed.outcome.custom_name, Some(Some("Typed".into())));
            assert_eq!(take_rename(&runtime, &player).as_deref(), Some("Typed"));
            assert_eq!(renamed.outcome.damage, None);
            assert_eq!(renamed.outcome.material_used, 0);
            assert_eq!(
                protocol::item_stack_damage(&renamed.stack),
                Some(damage as u32)
            );
            assert_eq!(renamed.outcome.cost, 1);
        }
    }
}

#[test]
fn unchanged_prefilled_names_keep_whitespace_and_do_not_charge_for_a_rename() {
    for name in [" Blade ", "   "] {
        let (mut runtime, mut player) = anvil();
        edit_input(
            &mut runtime,
            &mut player,
            ANVIL_TARGET_SLOT,
            protocol::ItemNbtEdit {
                custom_name: Some(Some(name.into())),
                ..Default::default()
            },
        );
        runtime.sync_anvil_name(&player);
        let preview = runtime.anvil_preview(&player).unwrap();
        assert_eq!(preview.outcome.custom_name, None);
        assert_eq!(preview.outcome.cost, 3);
        assert_eq!(
            protocol::item_custom_name(&preview.stack.extra_data).as_deref(),
            Some(name)
        );
        assert_eq!(
            take_rename(&runtime, &player).as_deref(),
            Some(name),
            "an unchanged custom name is restated so the server keeps it"
        );
        place(
            &mut runtime,
            &mut player,
            u16::from(ANVIL_MATERIAL_SLOT),
            NetworkItemStack::empty(),
        );
        assert!(
            runtime.anvil_preview(&player).is_none(),
            "inserting an unchanged name does nothing"
        );
        runtime.screen_state_mut().anvil_name = " New ".into();
        let renamed = runtime.anvil_preview(&player).unwrap();
        assert_eq!(renamed.outcome.custom_name, Some(Some(" New ".into())));
        assert_eq!(renamed.outcome.cost, 1);
        assert_eq!(take_rename(&runtime, &player).as_deref(), Some(" New "));
        runtime.screen_state_mut().anvil_name.clear();
        assert_eq!(take_rename(&runtime, &player).as_deref(), Some(""));
    }
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
    runtime.screen_state_mut().anvil_name.clear();
    assert_eq!(
        take_rename(&runtime, &player).as_deref(),
        Some(""),
        "empty text differs from the default name and is sent"
    );
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
fn ui_result_slot_packets_publish_the_retained_server_preview() {
    for (predictable, repair_cost) in [(false, 0), (true, 0), (true, 63)] {
        let (mut runtime, mut player) = anvil();
        edit_input(
            &mut runtime,
            &mut player,
            ANVIL_TARGET_SLOT,
            protocol::ItemNbtEdit {
                damage: (!predictable).then_some(0),
                repair_cost: Some(repair_cost),
                ..Default::default()
            },
        );
        runtime.sync_anvil_name(&player);
        runtime.screen_state_mut().hover =
            Some(ui_screens::container_actions::InventoryCellHit::CraftOutput);
        let pending = runtime.inventory_ledger(&player).pending_request_count();
        let result = NetworkItemStack {
            network_id: DIAMOND,
            count: 2,
            stack_network_id: 70,
            ..NetworkItemStack::empty()
        };
        place(
            &mut runtime,
            &mut player,
            u16::from(protocol::CREATED_OUTPUT_SLOT),
            result.clone(),
        );
        assert_eq!(runtime.anvil_output(&player), Some(result.clone()));
        assert_eq!(
            crate::ui_runtime::presentation::publish::hovered_stack(&player, &runtime),
            Some((result.clone(), None))
        );
        assert_eq!(
            runtime.inventory_ledger(&player).pending_request_count(),
            pending
        );
        assert_eq!(
            runtime.inventory_ledger(&player).created_output_stack(),
            Some(&result)
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
                custom_name: Some(Some(" Blade ".into())),
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
        let tail = [2, 3, 0, b'l', b'v', b'l', 1, 0, 0];
        let end = extra
            .windows(tail.len())
            .position(|bytes| bytes == tail)
            .expect("the fixture has an enchantment level followed by its compound end")
            + tail.len()
            - 1;
        let mut field = vec![2, 6, 0];
        field.extend_from_slice(b"custom");
        field.extend_from_slice(&42i16.to_le_bytes());
        extra.splice(end..end, field);
        input.extra_data = extra.into();
        input.nbt_digest = Sha256::digest(&input.extra_data).into();
        assert_eq!(protocol::item_anvil_data(&input.extra_data), None);
        place(&mut runtime, &mut player, u16::from(slot), input);
        runtime.sync_anvil_name(&player);
        assert_eq!(
            runtime.screen_state().anvil_name,
            " Blade ",
            "unsupported enchantments do not block name resolution"
        );
        assert_eq!(take_rename(&runtime, &player).as_deref(), Some(" Blade "));
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

#[test]
fn filtered_names_follow_actual_anvil_name_edits() {
    use protocol::{
        StackRequestAction as Action, StackRequestContainer as Container, StackRequestSlot,
    };

    for typed in [None, Some("Renamed"), Some("")] {
        let (mut runtime, mut player) = anvil();
        edit_input(
            &mut runtime,
            &mut player,
            ANVIL_TARGET_SLOT,
            protocol::ItemNbtEdit {
                custom_name: Some(Some(" Raw blade ".into())),
                filtered_custom_name: Some(Some("Filtered blade".into())),
                ..Default::default()
            },
        );
        runtime.sync_anvil_name(&player);
        assert_eq!(runtime.screen_state().anvil_name, " Raw blade ");
        if let Some(name) = typed {
            runtime.screen_state_mut().anvil_name = name.into();
        }
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Creative);

        let input_slot = |slot| {
            let ledger = runtime.inventory_ledger(&player);
            StackRequestSlot {
                container: protocol::ui_slot_request_container(slot).unwrap(),
                slot,
                stack_network_id: ledger
                    .target_stack(InventoryTarget::Craft(slot))
                    .unwrap()
                    .stack_network_id,
            }
        };
        let target = input_slot(ANVIL_TARGET_SLOT);
        let material = input_slot(ANVIL_MATERIAL_SLOT);
        let input = runtime
            .inventory_ledger(&player)
            .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .unwrap();

        let expected_name = match typed {
            None => Some(" Raw blade "),
            Some("") => None,
            Some(name) => Some(name),
        };
        let expected_filtered = typed.is_none().then_some("Filtered blade");
        let expected_extra = protocol::edit_item_extra(
            &input.extra_data,
            &protocol::ItemNbtEdit {
                damage: Some(0),
                repair_cost: Some(1),
                custom_name: Some(expected_name.map(str::to_owned)),
                filtered_custom_name: Some(expected_filtered.map(str::to_owned)),
                ..Default::default()
            },
        )
        .unwrap();

        let preview = runtime.anvil_preview(&player).unwrap();
        assert_eq!(preview.outcome.material_used, 3);
        assert_eq!(
            preview.outcome.custom_name,
            typed.map(|name| (!name.is_empty()).then(|| name.to_owned()))
        );
        assert_eq!(
            (
                protocol::item_custom_name(&preview.stack.extra_data).as_deref(),
                protocol::item_filtered_custom_name(&preview.stack.extra_data).as_deref(),
            ),
            (expected_name, expected_filtered)
        );
        assert_eq!(preview.stack.extra_data, expected_extra);

        runtime.perform_pointer_action(
            &mut player,
            crate::ui_runtime::inventory_drag::PointerAction::Click(
                ui_screens::container_actions::InventoryCellHit::CraftOutput,
            ),
        );
        let ledger = runtime.inventory_ledger(&player);
        assert_eq!(ledger.cursor_stack().unwrap().extra_data, expected_extra);
        let (packet, requests) = ledger.pending_batch().unwrap().unwrap();
        assert_eq!(requests, 1);
        let request_id = ledger.pending_request_id().unwrap();
        // An untouched custom name is restated; the server reads no text as clearing it.
        let filter_strings = vec![typed.unwrap_or(" Raw blade ").to_owned()];
        let expected = protocol::item_stack_request_packet_filtered(
            request_id,
            &[
                Action::CraftRecipeOptional {
                    recipe_network_id: 0,
                    filtered_string_index: 0,
                },
                Action::Consume {
                    amount: 3,
                    source: material,
                },
                Action::Consume {
                    amount: 1,
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
            &filter_strings,
        )
        .unwrap();
        assert_eq!(packet, expected);
    }
}

#[test]
fn overlay_only_names_survive_until_an_actual_rename() {
    let (runtime, player) = anvil();
    let input = runtime
        .inventory_ledger(&player)
        .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
        .unwrap();
    assert_eq!(protocol::item_custom_name(&input.extra_data), None);
    assert_eq!(protocol::item_filtered_custom_name(&input.extra_data), None);

    for filtered in [None, Some(""), Some("Filtered server blade")] {
        let overlay = inventory::inventory_ledger::StackResponseOverlay {
            custom_name: Some(Arc::from("Server blade")),
            filtered_custom_name: filtered.map(Arc::from),
            durability_correction: None,
        };
        let corrected = protocol::edit_item_extra(
            &input.extra_data,
            &anvil_name_edit(&input.extra_data, Some(&overlay), None),
        )
        .unwrap();
        assert_eq!(
            protocol::item_custom_name(&corrected).as_deref(),
            Some("Server blade")
        );
        assert_eq!(
            protocol::item_filtered_custom_name(&corrected).as_deref(),
            filtered.filter(|name| !name.is_empty())
        );
        assert_eq!(protocol::item_extra_damage_value(&corrected), Some(1000));

        for name in [Some("Renamed".to_owned()), None] {
            let edited = protocol::edit_item_extra(
                &corrected,
                &anvil_name_edit(&corrected, Some(&overlay), Some(name.clone())),
            )
            .unwrap();
            assert_eq!(
                protocol::item_custom_name(&edited).as_deref(),
                name.as_deref()
            );
            assert_eq!(protocol::item_filtered_custom_name(&edited), None);
        }
    }
}

/// Typing the default name back over a custom name removes it in both the request and the
/// prediction, as the server does for a missing name.
#[test]
fn typing_the_default_name_removes_a_custom_name_everywhere() {
    let (mut runtime, mut player) = anvil();
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_MATERIAL_SLOT),
        NetworkItemStack::empty(),
    );
    runtime.sync_anvil_name(&player);
    let default = runtime.screen_state().anvil_name.clone();
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            custom_name: Some(Some("Blade".into())),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, "Blade");
    runtime.screen_state_mut().anvil_name = default;
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.take_name, None);
    assert_eq!(preview.outcome.custom_name, Some(None));
    assert_eq!(protocol::item_custom_name(&preview.stack.extra_data), None);
    assert_eq!(preview.outcome.cost, 1);
}
