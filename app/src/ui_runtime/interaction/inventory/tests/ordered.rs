//! Ordered releases must not end a gesture started by a later press.
use super::*;
use bevy::input::mouse::MouseButtonInput;

/// Runs the inventory adapters with an admitted inventory and a known stone stack.
pub(super) fn personal_app(count: u16) -> (App, Entity) {
    use super::super::super::{drive_chat_keyboard_input, drive_inventory_ui_actions};
    use bevy::input::{keyboard::KeyboardInput, mouse::MouseButtonInput};
    use inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT;
    use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};

    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let mut runtime = client_ui::test_support::inventory_session(&mut player);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    let entries = protocol::vanilla_item_registry();
    let network_id = entries
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:stone")
        .unwrap()
        .network_id;
    ledger.apply_registry(&protocol::ItemRegistryEvent { entries });
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = NetworkItemStack {
        network_id,
        stack_network_id: 7,
        count,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    runtime.toggle_inventory(&mut player, None);
    let ledger = runtime.inventory_ledger_mut(&mut player);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 0, 0],
        runtime_entity_id: -1,
    }));
    let presentation = UiPresentationRuntime::with_hud(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
    )
    .unwrap();
    let at = player_slot_point(&presentation, 0);
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<WindowEvent>()
        .add_message::<MouseButtonInput>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(player)
        .insert_resource(UiRuntimeResource(runtime))
        .insert_resource(UiPresentationResource(presentation))
        .add_systems(
            Update,
            (drive_chat_keyboard_input, drive_inventory_ui_actions).chain(),
        );
    let window = app
        .world_mut()
        .spawn((window(at), CursorOptions::default(), PrimaryWindow))
        .id();
    (app, window)
}

/// Resolves a fallback player slot to the center of its window-space hitbox.
fn player_slot_point(presentation: &UiPresentationRuntime, index: u8) -> Vec2 {
    use client_ui::ui_runtime::presentation::screens;
    let gui_size = presentation
        .inventory_gui_point(
            UiPoint::new(SIZE[0] as f32, SIZE[1] as f32).unwrap(),
            SIZE,
            1.0,
        )
        .unwrap();
    let origin = screens::panel_origin(InventoryScreen::Personal, gui_size);
    let slot = screens::screen_slots(InventoryScreen::Personal)
        .into_iter()
        .find(|slot| slot.hit == InventoryCellHit::Player(index))
        .unwrap();
    window_point(
        presentation,
        [origin[0] + slot.pos[0] + 1.0, origin[1] + slot.pos[1] + 1.0],
    )
}

/// Publishes a physical edge to the ordered window stream and inventory release reader.
fn edge(app: &mut App, window: Entity, state: ButtonState) {
    let event = MouseButtonInput {
        button: MouseButton::Right,
        state,
        window,
    };
    let mut buttons = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
    match state {
        ButtonState::Pressed => buttons.press(MouseButton::Right),
        ButtonState::Released => buttons.release(MouseButton::Right),
    }
    app.world_mut().write_message(event);
    app.world_mut()
        .write_message(WindowEvent::MouseButtonInput(event));
}

/// Moves the inventory pointer to another player cell without pressing a button.
fn point_at(app: &mut App, window: Entity, slot: u8) {
    let at = player_slot_point(&app.world().resource::<UiPresentationResource>().0, slot);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(at));
}

#[test]
fn earlier_inventory_release_does_not_end_a_later_drag() {
    let (mut app, window) = personal_app(8);
    app.world_mut()
        .resource_scope(|world, mut player: Mut<client_state::PlayerRuntime>| {
            world
                .resource_mut::<UiRuntimeResource>()
                .perform_pointer_action(
                    &mut player,
                    client_ui::ui_runtime::inventory_drag::PointerAction::Click(
                        InventoryCellHit::Player(0),
                    ),
                );
        });
    point_at(&mut app, window, 1);
    edge(&mut app, window, ButtonState::Pressed);
    app.update();
    point_at(&mut app, window, 2);
    edge(&mut app, window, ButtonState::Released);
    edge(&mut app, window, ButtonState::Pressed);
    app.update();
    point_at(&mut app, window, 3);
    app.update();
    let player = app.world().resource::<client_state::PlayerRuntime>();
    assert_eq!(
        player
            .inventory
            .ledger()
            .displayed_stack(3)
            .map(|stack| stack.count),
        Some(1)
    );
}
