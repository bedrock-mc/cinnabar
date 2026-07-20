//! Authoritative gameplay-HUD state coverage: effects, metadata, armor,
//! mounts, hotbar/offhand mirroring, and the derived heart variants.

use protocol::{
    ActorEffectAction, ActorEffectEvent, ActorHandedness, ActorMetadata, ActorMetadataValue,
    ArmorEquipmentEvent, ContainerIdentity, EquipmentEvent, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent, NetworkItemStack, SelectedSlotEvent, SlotIdentity,
};
use sha2::Digest;

use super::*;
use crate::ui_runtime::gameplay_hud::{HeartVariant, MAX_HUD_EFFECTS};

fn effect(
    action: ActorEffectAction,
    effect_id: i32,
    duration_ticks: i32,
    tick: u64,
) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 1,
        action,
        effect_id,
        amplifier: 0,
        particles: true,
        ambient: false,
        duration_ticks,
        tick,
    }
}

fn stack(network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: -1,
        count: 1,
        nbt_digest: sha2::Sha256::digest([]).into(),
        block_runtime_id: 0,
        extra_data: std::sync::Arc::from([]),
    }
}

fn inventory_container(window_id: i32) -> ContainerIdentity {
    ContainerIdentity {
        window_id: Some(window_id),
        slot_type: None,
        dynamic_id: None,
    }
}

#[test]
fn local_effects_metadata_armor_and_mount_fan_into_gameplay_hud_state() {
    let mut runtime = UiRuntime::new(4);

    runtime
        .apply_local_effect(4, 1, effect(ActorEffectAction::Add, 19, 600, 100))
        .unwrap();
    runtime
        .apply_local_effect(4, 2, effect(ActorEffectAction::Add, 1, -1, 100))
        .unwrap();
    assert_eq!(runtime.gameplay_hud().effects().len(), 2);
    assert_eq!(
        runtime.gameplay_hud().heart_variant(Some(500)),
        HeartVariant::Poisoned
    );
    // The poison effect expires on the authoritative clock; the infinite
    // speed effect stays.
    assert_eq!(
        runtime.gameplay_hud().heart_variant(Some(701)),
        HeartVariant::Normal
    );

    runtime
        .apply_local_metadata(
            4,
            3,
            &[
                ActorMetadata {
                    key: 7,
                    value: ActorMetadataValue::Short(150),
                },
                ActorMetadata {
                    key: 42,
                    value: ActorMetadataValue::Short(300),
                },
                ActorMetadata {
                    key: 120,
                    value: ActorMetadataValue::Float(1.0),
                },
            ],
        )
        .unwrap();
    assert_eq!(runtime.gameplay_hud().air_ticks(), Some((150, 300)));
    assert_eq!(
        runtime
            .hud()
            .air()
            .map(|air| (air.current(), air.maximum())),
        Some((150, 300))
    );
    // Full freezing wins over the (expired) poison recolor.
    assert_eq!(
        runtime.gameplay_hud().heart_variant(Some(500)),
        HeartVariant::Frozen
    );

    runtime
        .apply_local_armor(
            4,
            4,
            &ArmorEquipmentEvent {
                actor_runtime_id: 1,
                helmet: stack(100),
                chestplate: NetworkItemStack::empty(),
                leggings: NetworkItemStack::empty(),
                boots: stack(101),
                body: NetworkItemStack::empty(),
            },
        )
        .unwrap();
    let armor = runtime.gameplay_hud().armor().expect("armor retained");
    assert_eq!(armor.helmet.network_id, 100);
    assert!(armor.chestplate.is_empty());

    runtime.apply_local_mount(4, 5, Some(-9)).unwrap();
    assert_eq!(runtime.gameplay_hud().mount_unique_id(), Some(-9));
    runtime.apply_local_mount(4, 6, None).unwrap();
    assert_eq!(runtime.gameplay_hud().mount_unique_id(), None);

    // Session replacement clears every retained gameplay-HUD surface.
    runtime.begin_session(5);
    assert!(runtime.gameplay_hud().effects().is_empty());
    assert_eq!(runtime.gameplay_hud().armor(), None);
    assert_eq!(runtime.gameplay_hud().air_ticks(), None);
    assert_eq!(runtime.gameplay_hud().mount_unique_id(), None);
}

#[test]
fn stale_local_gameplay_events_fail_without_mutation() {
    let mut runtime = UiRuntime::new(4);
    runtime
        .apply_local_effect(4, 10, effect(ActorEffectAction::Add, 19, 600, 100))
        .unwrap();
    assert!(matches!(
        runtime.apply_local_effect(4, 10, effect(ActorEffectAction::Add, 20, 600, 100)),
        Err(UiRuntimeError::StaleFifoSequence { .. })
    ));
    assert!(matches!(
        runtime.apply_local_mount(3, 11, Some(1)),
        Err(UiRuntimeError::WrongSession { .. })
    ));
    assert_eq!(runtime.gameplay_hud().effects().len(), 1);
}

#[test]
fn wither_outranks_poison_and_unknown_effect_actions_are_counted() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_effect(1, 1, effect(ActorEffectAction::Add, 19, -1, 0))
        .unwrap();
    runtime
        .apply_local_effect(1, 2, effect(ActorEffectAction::Add, 20, -1, 0))
        .unwrap();
    assert_eq!(
        runtime.gameplay_hud().heart_variant(None),
        HeartVariant::Withered
    );
    runtime
        .apply_local_effect(1, 3, effect(ActorEffectAction::Unknown(9), 21, -1, 0))
        .unwrap();
    assert_eq!(
        runtime.gameplay_hud().diagnostics().skipped_effect_actions,
        1
    );
    assert_eq!(runtime.gameplay_hud().effects().len(), 2);

    // Removal restores the poison recolor, then normal.
    runtime
        .apply_local_effect(1, 4, effect(ActorEffectAction::Remove, 20, 0, 0))
        .unwrap();
    assert_eq!(
        runtime.gameplay_hud().heart_variant(None),
        HeartVariant::Poisoned
    );
}

#[test]
fn effect_retention_is_bounded_with_soonest_expiry_eviction() {
    let mut runtime = UiRuntime::new(1);
    for index in 0..MAX_HUD_EFFECTS as i32 {
        runtime
            .apply_local_effect(
                1,
                1 + index as u64,
                effect(ActorEffectAction::Add, 100 + index, 100 + index, 0),
            )
            .unwrap();
    }
    assert_eq!(runtime.gameplay_hud().effects().len(), MAX_HUD_EFFECTS);
    runtime
        .apply_local_effect(
            1,
            1 + MAX_HUD_EFFECTS as u64,
            effect(ActorEffectAction::Add, 999, -1, 0),
        )
        .unwrap();
    assert_eq!(runtime.gameplay_hud().effects().len(), MAX_HUD_EFFECTS);
    assert_eq!(runtime.gameplay_hud().diagnostics().evicted_effects, 1);
    // The soonest-expiring effect (id 100) was evicted; the new one is present.
    assert!(
        runtime
            .gameplay_hud()
            .effects()
            .iter()
            .any(|effect| effect.effect_id == 999)
    );
    assert!(
        !runtime
            .gameplay_hud()
            .effects()
            .iter()
            .any(|effect| effect.effect_id == 100)
    );
}

#[test]
fn offhand_equipment_echo_does_not_clobber_the_main_hand_slot() {
    let mut runtime = UiRuntime::new(1);
    runtime.retain_local_selected_equipment(
        1,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(50),
            inventory_slot: 2,
            selected_slot: 2,
            window_id: 0,
            handedness: Some(ActorHandedness::Right),
        },
    );
    assert_eq!(runtime.selected_hotbar_slot(), Some(2));

    runtime.retain_local_selected_equipment(
        2,
        EquipmentEvent {
            actor_runtime_id: 7,
            stack: stack(60),
            inventory_slot: 0,
            selected_slot: 0,
            window_id: 119,
            handedness: Some(ActorHandedness::Left),
        },
    );
    // The offhand echo landed in the offhand mirror, not the slot echo.
    assert_eq!(runtime.selected_hotbar_slot(), Some(2));
    assert_eq!(
        runtime
            .gameplay_hud()
            .offhand_stack()
            .map(|stack| stack.network_id),
        Some(60)
    );
}

#[test]
fn inventory_content_slot_and_forced_selection_mirror_into_the_hotbar() {
    let mut runtime = UiRuntime::new(1);
    let mut slots = vec![NetworkItemStack::empty(); 36];
    slots[0] = stack(11);
    slots[8] = stack(19);
    runtime
        .enqueue_inventory_event(
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: inventory_container(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime
        .enqueue_inventory_event(
            1,
            2,
            InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: inventory_container(0),
                    slot: 3,
                },
                stack: stack(14),
                storage_item: None,
            }),
        )
        .unwrap();
    runtime.set_local_selected_slot(1);
    runtime
        .enqueue_inventory_event(
            1,
            3,
            InventoryEvent::SelectedSlot(SelectedSlotEvent {
                container: inventory_container(0),
                slot: 3,
                select_slot: true,
            }),
        )
        .unwrap();
    // Nothing is visible until the frame drain runs.
    assert!(!runtime.gameplay_hud().hotbar_known());
    runtime.drain_pending_inventory();

    assert!(runtime.gameplay_hud().hotbar_known());
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(0)
            .map(|stack| stack.network_id),
        Some(11)
    );
    assert_eq!(
        runtime
            .gameplay_hud()
            .hotbar_stack(3)
            .map(|stack| stack.network_id),
        Some(14)
    );
    assert_eq!(runtime.gameplay_hud().hotbar_stack(1), None);
    // The server-forced selection replaced the older local prediction.
    assert_eq!(runtime.selected_hotbar_slot(), Some(3));
    assert_eq!(
        runtime.selected_stack().map(|stack| stack.network_id),
        Some(14)
    );

    // The selected-item identity clock arms once the stack is known.
    runtime.observe_selected_item_identity(1_000);
    assert_eq!(runtime.selected_item_changed_millis(), Some(1_000));
    runtime.observe_selected_item_identity(2_000);
    assert_eq!(runtime.selected_item_changed_millis(), Some(1_000));
    runtime.set_local_selected_slot(0);
    runtime.observe_selected_item_identity(3_000);
    assert_eq!(runtime.selected_item_changed_millis(), Some(3_000));
}

#[test]
fn odd_metadata_values_are_counted_and_skipped_without_disconnect() {
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply_local_metadata(
            1,
            1,
            &[
                ActorMetadata {
                    key: 42,
                    value: ActorMetadataValue::Short(0),
                },
                ActorMetadata {
                    key: 120,
                    value: ActorMetadataValue::Float(f32::NAN),
                },
                ActorMetadata {
                    key: 7,
                    value: ActorMetadataValue::Int(5),
                },
            ],
        )
        .unwrap();
    assert_eq!(runtime.gameplay_hud().diagnostics().odd_metadata_values, 3);
    assert_eq!(runtime.gameplay_hud().air_ticks(), None);
    assert_eq!(runtime.gameplay_hud().freezing_strength(), 0.0);
}
