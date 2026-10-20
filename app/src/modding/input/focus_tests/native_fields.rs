//! Native fields preserve ordered composition and live text across app updates.

use super::*;

/// Platform composition stays inline until commit and unidentified keyboard text is discarded.
#[test]
fn native_text_platform_ime_commits_without_unidentified_leaks() {
    let component = fixture_with_panel(
        r#"{"title":"Search","toggle_key":"F10","dark":true,"controls":[{"kind":"text","id":"name","label":"Search","value":"Main","live":true,"navigation_keys":true}]}"#,
    );
    let (mut app, window) = personal_panel_app_with_fixture(false, &component);
    app.add_message::<Ime>();
    open_rendered_field(&mut app);
    app.update();
    assert!(
        app.world()
            .entity(window)
            .get::<Window>()
            .unwrap()
            .ime_enabled
    );
    app.world_mut().write_message(Ime::Preedit {
        window,
        value: "日本".into(),
        cursor: Some((3, 3)),
    });
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
        logical_key: Key::Character("leak".into()),
        state: ButtonState::Pressed,
        text: Some("leak".into()),
        repeat: false,
        window,
    });
    app.update();
    assert!(
        matches!(&app.world().resource::<ModRuntime>().hosts.host(0).panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == "Main")
    );
    app.world_mut().write_message(Ime::Commit {
        window,
        value: "日本".into(),
    });
    app.update();
    assert!(
        matches!(&app.world().resource::<ModRuntime>().hosts.host(0).panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == "日本")
    );
    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .focused = false;
    app.update();
    assert!(
        !app.world()
            .entity(window)
            .get::<Window>()
            .unwrap()
            .ime_enabled
    );
}

/// A submit followed by more typing in the same physical frame retains the newest live value.
#[test]
fn native_text_submit_then_typing_keeps_latest_value() {
    let component = fixture_with_panel(
        r#"{"title":"Search","toggle_key":"F10","dark":true,"controls":[{"kind":"text","id":"name","label":"Search","value":"Main","live":true,"navigation_keys":true}]}"#,
    );
    let (mut app, window) = personal_panel_app_with_fixture(false, &component);
    open_rendered_field(&mut app);
    for (key_code, text) in [
        (KeyCode::KeyA, Some("a")),
        (KeyCode::Enter, None),
        (KeyCode::KeyB, Some("b")),
    ] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state: ButtonState::Pressed,
            text: text.map(Into::into),
            repeat: false,
            window,
        });
    }
    app.update();
    assert!(
        matches!(&app.world().resource::<ModRuntime>().hosts.host(0).panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == "ab")
    );
}

/// Ordered window events keep the keystroke after a composition commit in the same update.
#[test]
fn native_text_orders_ime_and_keyboard_in_one_update() {
    let component = fixture_with_panel(
        r#"{"title":"Search","toggle_key":"F10","dark":true,"controls":[{"kind":"text","id":"name","label":"Search","value":"Main","live":true}]}"#,
    );
    let (mut app, window) = personal_panel_app_with_fixture(false, &component);
    app.add_message::<WindowEvent>();
    open_rendered_field(&mut app);
    app.world_mut()
        .write_message(WindowEvent::Ime(Ime::Preedit {
            window,
            value: "ka".into(),
            cursor: Some((2, 2)),
        }));
    app.update();
    let key = |key_code, text: &str| {
        WindowEvent::KeyboardInput(KeyboardInput {
            key_code,
            logical_key: Key::Character(text.into()),
            state: ButtonState::Pressed,
            text: Some(text.into()),
            repeat: false,
            window,
        })
    };
    app.world_mut().write_message(key(KeyCode::KeyQ, "discard"));
    app.world_mut().write_message(WindowEvent::Ime(Ime::Commit {
        window,
        value: "か".into(),
    }));
    app.world_mut().write_message(key(KeyCode::KeyA, "a"));
    app.update();
    assert!(
        matches!(&app.world().resource::<ModRuntime>().hosts.host(0).panel().unwrap().controls[0], ui::mod_panel::Control::Text { value, .. } if value == "かa")
    );
}
