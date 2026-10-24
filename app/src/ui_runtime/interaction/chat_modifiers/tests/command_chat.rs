//! The command shortcut opens the editor with its prefix and consumes the opening key.

use super::*;
use bevy::window::CursorGrabMode;

#[test]
fn slash_opens_chat_with_one_prefix_and_typing_continues_after_it() {
    command_chat_opens(KeyCode::Slash, false);
    command_chat_opens(KeyCode::Slash, true);
}

#[test]
fn french_azerty_slash_opens_command_chat() {
    command_chat_opens(KeyCode::Period, true);
    command_chat_opens(KeyCode::Period, false);
}

/// Exercises opening, editing, cancelling and returning to normal chat.
fn command_chat_opens(slash_key: KeyCode, shift: bool) {
    for sleeping in [false, true] {
        let mut h = Harness::new();
        h.app
            .world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .inventory
            .publish_local_runtime_id(1, 42)
            .unwrap();
        h.app
            .world_mut()
            .resource_mut::<UiRuntimeResource>()
            .set_local_sleeping(sleeping);
        if shift {
            h.press(KeyCode::ShiftLeft, None);
        }
        h.press(slash_key, Some("/"));

        if slash_key == KeyCode::Period && shift && !sleeping {
            snapshot(&h, "azerty-command-chat");
        }

        assert!(h.app.world().resource::<UiRuntimeResource>().chat_focused());
        assert_eq!(h.text(), "/");
        assert!(
            h.app
                .world()
                .resource::<UiRuntimeResource>()
                .pending_chat_sends()
                .is_empty()
        );
        let cursor = h.app.world().get::<CursorOptions>(h.window).unwrap();
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
        assert!(cursor.visible);
        assert!(
            h.app
                .world()
                .resource::<ButtonInput<KeyCode>>()
                .get_pressed()
                .next()
                .is_none()
        );

        h.release(slash_key);
        if shift {
            h.release(KeyCode::ShiftLeft);
        }
        h.press(KeyCode::KeyH, Some("help"));
        assert_eq!(h.text(), "/help");
        h.press(KeyCode::Escape, None);
        assert!(!h.app.world().resource::<UiRuntimeResource>().chat_focused());
        assert_eq!(h.text(), "");
        h.press(KeyCode::KeyT, Some("t"));
        assert!(h.app.world().resource::<UiRuntimeResource>().chat_focused());
        assert_eq!(h.text(), "");
    }
}

/// Captures the input system's output with installed UI and font carriers when requested.
fn snapshot(h: &Harness, name: &str) {
    let Ok(path) = std::env::var("CINNABAR_FONT_SNAPSHOT_CARRIER") else {
        return;
    };
    let manifest = assets::canonical_source_manifest_sha256(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../assets/cinnangles-sans-source.json"),
        )
        .unwrap(),
    );
    let font = assets::RuntimeFontCatalog::decode(&std::fs::read(path).unwrap(), manifest)
        .unwrap()
        .with_coverage_pages();
    let Some(mut presentation) =
        client_ui::test_support::engine_presentation_with(std::sync::Arc::new(font))
    else {
        eprintln!("skipping command chat snapshot: missing installed UI carrier; make assets");
        return;
    };
    let input = presentation
        .build(
            h.app.world().resource::<client_state::PlayerRuntime>(),
            h.app.world().resource::<UiRuntimeResource>(),
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    client_ui::ui_runtime::presentation::forms::snapshot::write(&input, name);
}

#[test]
fn slash_in_open_chat_is_inserted_at_the_caret() {
    let mut h = Harness::new();
    h.press(KeyCode::KeyT, Some("t"));
    h.press(KeyCode::KeyA, Some("ab"));
    h.press(KeyCode::ArrowLeft, None);
    h.press(KeyCode::Slash, Some("/"));
    assert_eq!(h.text(), "a/b");
}

#[test]
fn unfocused_slash_does_not_open_chat_or_replay_after_focus_returns() {
    let mut h = Harness::new();
    h.set_focused(false);
    h.press(KeyCode::Slash, Some("/"));
    h.set_focused(true);
    h.app.update();
    assert!(!h.app.world().resource::<UiRuntimeResource>().chat_focused());
    assert_eq!(h.text(), "");
}

#[test]
fn typed_slash_respects_remapped_or_unbound_command_control() {
    use launcher::menu::settings_options::{
        EXTRA_KEYS, KEY_BINDINGS, SETTINGS_FILE, SettingsOptions,
    };

    let row = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.command")
            .unwrap();
    for remap in [true, false] {
        let layout = launcher::test_support::scratch("command-chat-binding");
        let mut settings = SettingsOptions::default();
        if remap {
            assert!(
                settings.remap(
                    row,
                    semantic_input::KeyboardKey::KeyY
                        .usage()
                        .map(semantic_input::PhysicalControl::KeyboardUsage)
                        .unwrap()
                )
            );
        } else {
            assert!(settings.unbind_key(row));
        }
        settings
            .save(&layout.server_file().with_file_name(SETTINGS_FILE))
            .unwrap();
        let menu = client_shell::menu::MenuRuntime::new_with_layout(
            false,
            Some(2),
            "Fixture".into(),
            layout,
            client_shell::player_skin::LocalPlayerSkin::generated_default("Fixture"),
        );
        let mut h = Harness::new();
        h.app.insert_resource(menu);
        h.press(KeyCode::ShiftLeft, None);
        h.press(KeyCode::Period, Some("/"));
        assert!(!h.app.world().resource::<UiRuntimeResource>().chat_focused());
        h.release(KeyCode::ShiftLeft);
        h.press(KeyCode::KeyY, Some("y"));
        assert_eq!(
            h.app.world().resource::<UiRuntimeResource>().chat_focused(),
            remap
        );
        assert_eq!(h.text(), if remap { "/" } else { "" });
    }
}
