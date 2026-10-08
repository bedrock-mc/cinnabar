//! Same-frame pointer and close-key ordering through the production input owners.

use crate::player_runtime::PlayerRuntime;
use std::sync::Arc;

use bevy::{
    input::{
        ButtonInput, ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
        mouse::{AccumulatedMouseMotion, MouseButtonInput},
        touch::Touches,
    },
    prelude::{App, Entity, IntoScheduleConfigs, MouseButton, Update},
    time::{Real, Time},
    window::{CursorOptions, PrimaryWindow, Window, WindowEvent},
};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, NetworkItemStack,
};
use ui::UiPoint;

use crate::{
    app::{ClientFrameSet, configure_client_frame_schedule},
    menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    ui_runtime::{
        apply_deferred_inventory_close, drive_chat_keyboard_input, drive_inventory_ui_actions,
        presentation::tests::fixture_font,
    },
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Opens the personal inventory over one stack in slot zero, with the pointer on that slot.
fn app() -> (App, Entity) {
    let mut player_runtime = PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(&mut player_runtime, InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from(
                (0..36)
                    .map(|index| NetworkItemStack {
                        network_id: if index == 0 { 6 } else { 0 },
                        count: if index == 0 { 2 } else { 0 },
                        stack_network_id: if index == 0 { 55 } else { 0 },
                        ..NetworkItemStack::default()
                    })
                    .collect::<Vec<_>>(),
            ),
            storage_item: NetworkItemStack::default(),
        }));
    runtime.toggle_inventory(&mut player_runtime);
    assert!(
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .mark_transport_enqueued(0)
    );
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: -1,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }));

    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let physical_size = [1280, 720];
    let pointer = (0..physical_size[1])
        .find_map(|y| {
            (0..physical_size[0]).find_map(|x| {
                let point = UiPoint::new(x as f32, y as f32).unwrap();
                let gui = presentation.inventory_gui_point(point, physical_size, 1.0)?;
                (presentation.inventory_slot_hit(gui, physical_size, 1.0) == Some(0))
                    .then_some(bevy::math::Vec2::new(x as f32, y as f32))
            })
        })
        .expect("slot zero has a physical hit point");
    let mut window = Window {
        focused: true,
        resolution: bevy::window::WindowResolution::new(1280, 720),
        ..Default::default()
    };
    window.set_cursor_position(Some(pointer));

    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .add_message::<KeyboardInput>()
        .add_message::<WindowEvent>()
        .insert_resource(runtime)
        .insert_resource(player_runtime)
        .insert_resource(presentation)
        .insert_resource(MenuRuntime::new(false, 2, "Tester".to_owned()))
        .add_systems(
            Update,
            (
                drive_chat_keyboard_input,
                drive_menu_input,
                drive_inventory_ui_actions,
                apply_deferred_inventory_close,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        );
    let window = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    (app, window)
}

/// Delivers a primary click and Escape in one frame, in the given window order.
fn click_and_escape(app: &mut App, window: Entity, click_first: bool) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    let escape = KeyboardInput {
        key_code: KeyCode::Escape,
        logical_key: Key::Escape,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    };
    let click = WindowEvent::MouseButtonInput(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window,
    });
    let key = WindowEvent::KeyboardInput(escape.clone());
    let ordered = if click_first {
        [click, key]
    } else {
        [key, click]
    };
    for event in ordered {
        app.world_mut().write_message(event);
    }
    app.world_mut().write_message(escape);
}

/// A click that reached the window before Escape lands in the screen before it closes.
#[test]
fn click_then_escape_in_one_frame_applies_the_click() {
    for click_first in [true, false] {
        let (mut app, window) = app();
        click_and_escape(&mut app, window, click_first);

        app.update();

        assert!(!app.world().resource::<UiRuntime>().inventory_open());
        let ledger = app.world().resource::<PlayerRuntime>().inventory.ledger();
        assert_eq!(
            ledger.pending_request_id().is_some(),
            click_first,
            "click_first={click_first}: only a click before Escape picks up the stack"
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left),
            "click_first={click_first}: the click never reaches gameplay"
        );
    }
}
