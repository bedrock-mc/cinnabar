//! Closing-frame wheel ownership covers raw inventory consumers after the wheel.
use super::*;
use crate::ui_runtime::interaction::drive_world_inventory_keys;
use client_ui::ui_runtime::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT;
use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, ItemRegistryEvent, NetworkItemStack,
};
use std::sync::Arc;

fn inventory_harness(identifier: &str) -> Harness {
    let mut h = Harness::new();
    h.app.add_systems(
        Update,
        drive_world_inventory_keys
            .after(crate::ui_runtime::interaction::drive_chat_keyboard_input)
            .before(finalize_semantic_input_after_ui_authority),
    );
    let entries = protocol::vanilla_item_registry();
    let network_id = entries
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .network_id;
    h.app
        .world_mut()
        .resource_scope(|world, mut runtime: Mut<UiRuntime>| {
            let mut player = world.resource_mut::<PlayerRuntime>();
            player
                .facts
                .publish_player_game_mode(protocol::PlayerGameMode::Survival);
            runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
            let ledger = runtime.inventory_ledger_mut(&mut player);
            ledger.apply_registry(&ItemRegistryEvent { entries });
            let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
            slots[0] = NetworkItemStack {
                network_id,
                count: 2,
                stack_network_id: 7,
                ..NetworkItemStack::default()
            };
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: Arc::from(slots),
                storage_item: NetworkItemStack::empty(),
            }));
        });
    h
}

#[test]
fn closing_emote_wheel_cannot_replay_controller_navigation_as_world_drop() {
    for closing_button in [GamepadButton::South, GamepadButton::East] {
        let mut h = inventory_harness("minecraft:stone");
        let pad = h.app.world_mut().spawn(Gamepad::default()).id();
        h.press(KeyCode::KeyB);
        h.app
            .world_mut()
            .resource_mut::<UiRuntime>()
            .emotes_mut()
            .hover_slot(Some(0));
        {
            let mut controller = h.app.world_mut().get_mut::<Gamepad>(pad).unwrap();
            controller.digital_mut().press(GamepadButton::DPadDown);
            controller.digital_mut().press(closing_button);
        }
        h.app.update();
        assert!(!h.runtime().emotes().is_open());
        assert!(h.app.world().resource::<EmoteInputConsumed>().0);
        assert_eq!(
            h.app
                .world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .pending_request_count(),
            0
        );
        assert_eq!(
            h.app
                .world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .displayed_stack(0)
                .unwrap()
                .count,
            2
        );
        // The next independently pressed drop is still admitted through the same system.
        h.app
            .world_mut()
            .get_mut::<Gamepad>(pad)
            .unwrap()
            .digital_mut()
            .reset_all();
        h.app.update();
        h.app
            .world_mut()
            .get_mut::<Gamepad>(pad)
            .unwrap()
            .digital_mut()
            .press(GamepadButton::DPadDown);
        h.app.update();
        assert!(!h.app.world().resource::<EmoteInputConsumed>().0);
        assert_eq!(
            h.app
                .world()
                .resource::<PlayerRuntime>()
                .inventory
                .ledger()
                .pending_request_count(),
            1
        );
    }
}

#[test]
fn closing_emote_wheel_consumes_book_use_and_allows_a_fresh_gameplay_use() {
    let mut h = inventory_harness("minecraft:writable_book");
    h.press(KeyCode::KeyB);
    h.queue(KeyCode::Escape);
    h.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    h.app.update();
    assert!(!h.runtime().emotes().is_open());
    assert!(!h.runtime().inventory_open());
    assert!(h.app.world().resource::<EmoteInputConsumed>().0);
    h.app.update();
    h.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    h.app.update();
    assert!(!h.app.world().resource::<EmoteInputConsumed>().0);
    assert!(h.runtime().inventory_open());
    assert!(h.runtime().screen_state().book.is_some());
}
