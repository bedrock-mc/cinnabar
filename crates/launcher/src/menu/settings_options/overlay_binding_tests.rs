//! Overlay bindings persist, reset and drive the production player-list action.

use super::BindingId;
use super::*;
use semantic_input::{DeviceFrame, KeyboardMouseFrame, SemanticInputRouter};

/// Finds the settings row using its persisted binding identifier.
fn row(name: &str) -> usize {
    KEY_BINDINGS
        .iter()
        .map(|(_, name)| *name)
        .chain(EXTRA_KEYS.iter().map(|(name, _)| *name))
        .position(|candidate| candidate == name)
        .expect("overlay action has a settings row")
}

/// Sends one physical control through the gameplay router.
fn frame(control: PhysicalControl, activity_sequence: u64) -> DeviceFrame {
    let mut keyboard_mouse = KeyboardMouseFrame {
        activity_sequence,
        ..Default::default()
    };
    match control {
        PhysicalControl::KeyboardUsage(usage) => keyboard_mouse.keys.push(usage),
        PhysicalControl::MouseButton(button) => keyboard_mouse.mouse_buttons.push(button),
        _ => panic!("overlay controls are keyboard or mouse buttons"),
    }
    DeviceFrame {
        keyboard_mouse: Some(keyboard_mouse),
        ..Default::default()
    }
}

#[test]
fn overlay_bindings_keep_defaults_and_remap_reset_across_reload() {
    let mut settings = SettingsOptions::decode(br#"{"keys":{"key.drop":69}}"#).unwrap();
    for (name, label, default) in [
        (
            DEBUG_OVERLAY_KEY,
            "Debug Overlay",
            PhysicalControl::KeyboardUsage(0x3c),
        ),
        (
            PLAYER_LIST_KEY,
            "Player List",
            PhysicalControl::KeyboardUsage(0x2b),
        ),
    ] {
        assert_eq!(custom_key_label(name), Some(label));
        assert_eq!(
            settings.named_key_control(BindingId::from_name(name).expect("registered binding row")),
            Some(default)
        );
        for control in [
            PhysicalControl::KeyboardUsage(0x3d),
            PhysicalControl::MouseButton(4),
        ] {
            assert!(settings.remap(row(name), control));
            settings = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
            assert_eq!(
                settings
                    .named_key_control(BindingId::from_name(name).expect("registered binding row")),
                Some(control)
            );
        }
        assert!(settings.reset_key(row(name)));
        assert_eq!(
            settings.named_key_control(BindingId::from_name(name).expect("registered binding row")),
            Some(default)
        );
    }
    assert_eq!(
        settings.named_key_control(BindingId::Drop),
        Some(PhysicalControl::KeyboardUsage(69))
    );
    assert!(settings.remap(row(DEBUG_OVERLAY_KEY), PhysicalControl::MouseButton(5)));
    assert!(settings.remap(row(PLAYER_LIST_KEY), PhysicalControl::KeyboardUsage(0x3d)));
    settings.reset_bindings(true);
    assert_eq!(
        settings.named_key_control(BindingId::DebugOverlay),
        Some(PhysicalControl::MouseButton(5))
    );
    settings.reset_bindings(false);
    for name in [DEBUG_OVERLAY_KEY, PLAYER_LIST_KEY] {
        assert_eq!(
            settings.named_key_control(BindingId::from_name(name).expect("registered binding row")),
            SettingsOptions::default()
                .named_key_control(BindingId::from_name(name).expect("registered binding row"))
        );
    }
}

#[test]
fn remapped_player_list_holds_releases_and_leaves_ui_tab_navigation_intact() {
    let tab = PhysicalControl::KeyboardUsage(0x2b);
    for control in [
        PhysicalControl::KeyboardUsage(0x3d),
        PhysicalControl::MouseButton(4),
    ] {
        let mut settings = SettingsOptions::default();
        assert!(settings.remap(row(PLAYER_LIST_KEY), control));
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        let mut router = SemanticInputRouter::default();
        router.replace_bindings(loaded.controls().unwrap()).unwrap();
        router.route(frame(tab, 1)).unwrap();
        assert!(!router.finalize().unwrap().phases[Action::PlayerList as usize].held);
        router.route(frame(control, 2)).unwrap();
        let phase = router.finalize().unwrap().phases[Action::PlayerList as usize];
        assert!(phase.pressed && phase.held);
        router.route(frame(control, 2)).unwrap();
        let phase = router.finalize().unwrap().phases[Action::PlayerList as usize];
        assert!(phase.held && !phase.pressed);
        router.route(frame(tab, 3)).unwrap();
        let phase = router.finalize().unwrap().phases[Action::PlayerList as usize];
        assert!(phase.released && !phase.held);
        router.route(DeviceFrame::default()).unwrap();
        router.finalize().unwrap();
        router.set_context(InputContext::UiFocused);
        router.route(frame(tab, 4)).unwrap();
        let snapshot = router.finalize().unwrap();
        assert!(snapshot.phases[Action::UiTabNext as usize].pressed);
        assert!(!snapshot.phases[Action::PlayerList as usize].held);
    }
}
