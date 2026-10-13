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
    runtime
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
        32
    );
    assert_eq!(runtime.screen_state().anvil_name, prefilled);
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, None);
    assert_eq!(preview.outcome.cost, 3);
}
