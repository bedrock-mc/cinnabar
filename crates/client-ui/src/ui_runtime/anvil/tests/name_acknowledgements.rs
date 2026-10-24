use super::presentation::correct_target;
use super::*;

#[test]
fn echoing_an_nbt_name_and_restating_it_preserve_edits() {
    let (mut runtime, mut player) = anvil();
    let original = " Blade ";
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            custom_name: Some(Some(original.into())),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, original);
    assert!(
        runtime
            .inventory_ledger(&player)
            .presented_target_overlay(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .is_none()
    );
    runtime.screen_state_mut().anvil_name = "Edited".into();
    correct_target(&mut runtime, &mut player, 1000, original);
    assert_eq!(
        runtime
            .inventory_ledger(&player)
            .presented_target_overlay(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .unwrap()
            .custom_name
            .as_deref(),
        Some(original)
    );
    for _ in 0..2 {
        runtime.sync_anvil_name(&player);
        assert_eq!(
            runtime.screen_state().anvil_name,
            "Edited",
            "an echoed name is unchanged"
        );
    }
    let target = runtime
        .inventory_ledger(&player)
        .target_stack(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
        .unwrap()
        .clone();
    place(
        &mut runtime,
        &mut player,
        u16::from(ANVIL_TARGET_SLOT),
        target,
    );
    assert!(
        runtime
            .inventory_ledger(&player)
            .presented_target_overlay(InventoryTarget::Craft(ANVIL_TARGET_SLOT))
            .is_none()
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(
        runtime.screen_state().anvil_name,
        "Edited",
        "the same name in NBT is unchanged"
    );
    let preview = runtime.anvil_preview(&player).unwrap();
    assert_eq!(preview.outcome.custom_name, Some(Some("Edited".into())));
    assert_eq!(preview.outcome.cost, 4);
    assert_eq!(take_rename(&runtime, &player).as_deref(), Some("Edited"));
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            damage: Some(500),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    assert_eq!(
        runtime.screen_state().anvil_name,
        "Edited",
        "a damage restatement keeps the name"
    );
    assert_eq!(runtime.anvil_preview(&player).unwrap().outcome.cost, 3);
}

#[test]
fn a_changed_authoritative_name_still_refreshes_the_prefill() {
    let (mut runtime, mut player) = anvil();
    edit_input(
        &mut runtime,
        &mut player,
        ANVIL_TARGET_SLOT,
        protocol::ItemNbtEdit {
            custom_name: Some(Some("Original".into())),
            ..Default::default()
        },
    );
    runtime.sync_anvil_name(&player);
    runtime.screen_state_mut().anvil_name = "Edited".into();
    correct_target(&mut runtime, &mut player, 1000, "Changed");
    runtime.sync_anvil_name(&player);
    assert_eq!(runtime.screen_state().anvil_name, "Changed");
    assert_eq!(
        runtime.anvil_preview(&player).unwrap().outcome.custom_name,
        None
    );
    assert_eq!(take_rename(&runtime, &player).as_deref(), Some("Changed"));
}
