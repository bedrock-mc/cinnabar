use super::*;
use crate::modding::packets::PacketMods;

/// Starts a packet-only input app with a closed menu and the requested subscriptions.
fn packet_input_app(
    directory: &std::path::Path,
    subscriptions: &[&str],
) -> (App, Entity, std::sync::Arc<dyn protocol::PacketRelay>) {
    let path = directory.join("input.wat");
    let source = include_str!("../../../../crates/mod-host/src/packet_mod/tests/guest.wat")
        .replace("$IMPORTS", "")
        .replace("$START", "");
    let output = serde_json::json!({"open":false, "keys":subscriptions}).to_string();
    let encoded: String = output.bytes().map(|byte| format!("\\{byte:02x}")).collect();
    let mut source = source.trim_end().strip_suffix(')').unwrap().to_owned();
    source.push_str(&format!(
        r#"
        (data (i32.const 2048) "{encoded}")
        (func (export "native-init") (param i32))
        (func (export "menu") (param i32 i32) (result i32)
            (i32.store (i32.const 128) (i32.const 0))
            (i32.store (i32.const 132) (i32.const 2048))
            (i32.store (i32.const 136) (i32.const {}))
            (i32.const 128)))
    "#,
        output.len()
    ));
    std::fs::write(&path, source).unwrap();
    let mut packets = PacketMods::default();
    packets.select_environment(
        client_extensions::modding::packet_relay::PacketModSelection {
            path,
            grants: mod_host::ModGrants {
                packets: true,
                controls: true,
                ..Default::default()
            },
        },
    );
    let relay = packets.session_relay().unwrap();
    relay.start(
        &protocol::session_wire::SessionIdentity {
            display_name: "Fixture".into(),
            xuid: "1".into(),
            uuid: "fixture".into(),
        },
        "{}",
        &[],
    );
    assert!(packets.native_menu_identity().is_some());
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .add_message::<MouseButtonInput>()
        .init_resource::<crate::modding::ReservedControls>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(packets)
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(UiPresentationResource(
            client_ui::test_support::mini_engine_presentation(),
        ))
        .add_systems(Update, prepare_mod_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    test_time::eventually("packet mouse subscriptions", || {
        app.update();
        app.world()
            .resource::<PacketMods>()
            .menu
            .retained
            .as_ref()
            .is_some_and(|output| output.output.keys.len() == subscriptions.len())
    });
    (app, window, relay)
}

#[test]
fn packet_only_refocus_discards_stale_held_keys() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, window, relay) = packet_input_app(directory.path(), &[]);
    for key in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        app.world_mut().write_message(KeyboardInput {
            key_code: key,
            logical_key: bevy::input::keyboard::Key::Shift,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        app.update();
        assert!(app.world().resource::<PacketMods>().menu.pending.shift_held);
        assert!(!app.world().resource::<PacketMods>().menu_owns_panel());

        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert!(!app.world().resource::<PacketMods>().menu.pending.shift_held);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert!(
            !app.world().resource::<PacketMods>().menu.pending.shift_held,
            "refocus must resample physical input after a release outside the window"
        );
    }
    relay.stop();
}

#[test]
fn packet_mouse_subscriptions_route_edges_and_consume_only_reserved_buttons() {
    let directory = tempfile::tempdir().unwrap();
    let (mut app, window, relay) =
        packet_input_app(directory.path(), &POINTER_BUTTONS.map(|(_, name)| name));
    for fast_tap in [false, true] {
        for (button, name) in POINTER_BUTTONS {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyW);
            {
                let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
                mouse.press(MouseButton::Back);
                mouse.press(button);
                if fast_tap {
                    mouse.release(button);
                }
            }
            for _ in 0..2 {
                app.world_mut().write_message(MouseButtonInput {
                    button,
                    state: ButtonState::Pressed,
                    window,
                });
            }
            if fast_tap {
                app.world_mut().write_message(MouseButtonInput {
                    button,
                    state: ButtonState::Released,
                    window,
                });
            }
            app.update();
            let pending = &app.world().resource::<PacketMods>().menu.pending.keys;
            assert_eq!(pending.len(), 1, "{name}, fast tap: {fast_tap}");
            assert_eq!(pending[0].key, name);
            assert_eq!(pending[0].text, None);
            let mouse = app.world().resource::<ButtonInput<MouseButton>>();
            assert!(!mouse.pressed(button));
            assert!(!mouse.just_pressed(button));
            assert!(!mouse.just_released(button));
            assert!(mouse.pressed(MouseButton::Back));
            assert!(
                app.world()
                    .resource::<ButtonInput<KeyCode>>()
                    .pressed(KeyCode::KeyW)
            );
            app.update();
            assert!(
                app.world()
                    .resource::<PacketMods>()
                    .menu
                    .pending
                    .keys
                    .is_empty()
            );
            app.world_mut().write_message(MouseButtonInput {
                button,
                state: ButtonState::Released,
                window,
            });
            app.update();
        }
    }
    relay.stop();
}
