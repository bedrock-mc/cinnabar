//! Real keyboard and production ingress regression for close-response admission.

use crate::player_runtime::PlayerRuntime;
use bevy::{
    input::{
        ButtonInput, ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
        mouse::AccumulatedMouseMotion,
    },
    prelude::{App, Entity, IntoScheduleConfigs, MouseButton, Update},
    time::{Real, Time},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use protocol::{
    ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent,
};

use crate::ui_runtime::{drain_inventory_authority, drive_chat_keyboard_input};
use {
    client_ui::ui_runtime::{UiRuntime, flush_inventory_send},
    inventory::inventory_ledger::{GENERIC_STORAGE_WINDOW_TYPE, PERSONAL_INVENTORY_WINDOW_TYPE},
};

/// Builds the keyboard fixture with one live domain owner.
fn app(authority: InventoryAuthority) -> (App, Entity) {
    let mut player_runtime = PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(&mut player_runtime, authority);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .add_message::<KeyboardInput>()
        .insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .add_systems(
            Update,
            (drain_inventory_authority, drive_chat_keyboard_input).chain(),
        );
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Window::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    (app, window)
}

fn key(app: &mut App, window: Entity, key_code: KeyCode, state: ButtonState) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    match state {
        ButtonState::Pressed => keys.press(key_code),
        ButtonState::Released => keys.release(key_code),
    }
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: if key_code == KeyCode::Escape {
            Key::Escape
        } else {
            Key::Character("e".into())
        },
        state,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}

fn receive(app: &mut App, sequence: u64, event: InventoryEvent) {
    crate::tests::with_ui_player(app, |runtime, player| {
        runtime
            .enqueue_inventory_event(player, 1, sequence, event)
            .unwrap();
    });
    app.update();
}

fn flush(app: &mut App, millis: u64) {
    crate::tests::with_ui_player(app, |runtime, player| {
        assert!(flush_inventory_send(player, runtime, millis, |_| Ok::<_, ()>(())).unwrap());
        assert!(!flush_inventory_send(player, runtime, millis, |_| Ok::<_, ()>(())).unwrap());
    });
}

#[test]
fn e_key_reopens_after_e_or_escape_and_response_payload_does_not_identify_the_screen() {
    for authority in [InventoryAuthority::Server, InventoryAuthority::Client] {
        // The native response branch never inspects either payload
        // identifier. Include generic type/window0 (DF), an inventory type, a
        // sentinel and an odd but well-framed type, without asserting BDS's shape.
        for (response_window, response_type) in [
            (0, GENERIC_STORAGE_WINDOW_TYPE),
            (3, PERSONAL_INVENTORY_WINDOW_TYPE),
            (255, 127),
        ] {
            for close_key in [KeyCode::KeyE, KeyCode::Escape] {
                let (mut app, window) = app(authority);
                for cycle in 0..3 {
                    key(&mut app, window, KeyCode::KeyE, ButtonState::Pressed);
                    assert!(
                        app.world().resource::<UiRuntime>().inventory_open(),
                        "cycle {cycle}"
                    );
                    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
                    flush(&mut app, cycle * 100_000 + 10);
                    receive(
                        &mut app,
                        cycle * 2 + 1,
                        InventoryEvent::Open(ContainerOpenEvent {
                            container: ContainerIdentity::window(0),
                            window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
                            position: [0; 3],
                            runtime_entity_id: -1,
                        }),
                    );
                    key(&mut app, window, KeyCode::KeyE, ButtonState::Released);
                    key(&mut app, window, close_key, ButtonState::Pressed);
                    assert!(!app.world().resource::<UiRuntime>().inventory_open());
                    assert_eq!(
                        app.world().get::<CursorOptions>(window).unwrap().grab_mode,
                        CursorGrabMode::Locked
                    );
                    flush(&mut app, cycle * 100_000 + 20);
                    receive(
                        &mut app,
                        cycle * 2 + 2,
                        InventoryEvent::Close(ContainerCloseEvent {
                            container: ContainerIdentity::window(response_window),
                            window_type: response_type,
                            server_initiated: false,
                        }),
                    );
                    key(&mut app, window, close_key, ButtonState::Released);
                    crate::tests::with_ui_player(&mut app, |runtime, player_runtime| {
                        runtime.poll_inventory_timeout(player_runtime, cycle * 100_000 + 60_000);
                        assert!(!runtime.inventory_open());
                        assert!(
                            !runtime
                                .inventory_ledger(player_runtime)
                                .personal_inventory_desired_open()
                        );
                    });
                }
            }
        }
    }
}
