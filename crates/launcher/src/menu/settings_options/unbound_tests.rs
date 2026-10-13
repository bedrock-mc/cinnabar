use super::*;

use super::BindingId;
#[test]
fn explicit_unbound_survives_reload_and_preserves_other_device_and_actions() {
    let mut settings = SettingsOptions::default();
    let jump = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Jump)
        .unwrap();
    let original = settings.controls().unwrap();
    settings.keys.insert("key.jump".into(), UNBOUND_CONTROL);
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(loaded.key_control(jump), None);
    let controls = loaded.controls().unwrap();
    let expected = original
        .bindings()
        .iter()
        .copied()
        .filter(|binding| {
            binding.action != Action::Jump
                || !matches!(
                    binding.chord.control,
                    PhysicalControl::KeyboardUsage(_) | PhysicalControl::MouseButton(_)
                )
        })
        .collect::<Vec<_>>();
    assert_eq!(controls.bindings(), expected);
    let mut restored = loaded;
    assert!(restored.reset_key(jump));
    assert_eq!(
        restored.key_control(jump),
        SettingsOptions::default().key_control(jump)
    );
    assert_eq!(restored.controls().unwrap(), original);
}

#[test]
fn explicit_unbound_supplemental_key_removes_secondary_default() {
    let mut settings = SettingsOptions::default();
    settings.keys.insert("key.chat".into(), UNBOUND_CONTROL);
    let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(loaded.named_key_control(BindingId::Chat), None);
    assert_eq!(loaded.secondary_key_control(BindingId::Chat), None);
    loaded.reset_bindings(false);
    assert_eq!(
        loaded.named_key_control(BindingId::Chat),
        SettingsOptions::default().named_key_control(BindingId::Chat)
    );
    assert_eq!(
        loaded.secondary_key_control(BindingId::Chat),
        SettingsOptions::default().secondary_key_control(BindingId::Chat)
    );
}

#[test]
fn unbind_then_remap_and_device_reset_restore_keyboard_bindings() {
    let mut settings = SettingsOptions::default();
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Jump)
        .unwrap();
    assert!(!settings.unbind_key(GAMEPAD_OFFSET));
    assert!(!settings.unbind_key(KEY_BINDINGS.len() + EXTRA_KEYS.len()));
    assert!(settings.unbind_key(row));
    assert_eq!(settings.key_control(row), None);
    let mut router = semantic_input::SemanticInputRouter::default();
    router
        .replace_bindings(settings.controls().unwrap())
        .unwrap();
    router
        .route(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                activity_sequence: 1,
                keys: vec![0x2c],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(!router.finalize().unwrap().phases[Action::Jump as usize].held);
    assert!(settings.remap(row, PhysicalControl::KeyboardUsage(0x0d)));
    assert_eq!(
        settings.key_control(row),
        Some(PhysicalControl::KeyboardUsage(0x0d))
    );
    assert!(settings.unbind_key(row));
    settings.reset_bindings(false);
    assert_eq!(settings.controls().unwrap(), ControlSettings::default());
}

#[test]
fn explicit_empty_and_missing_bindings_are_distinct_but_invalid_codes_are_rejected() {
    assert_eq!(
        SettingsOptions::decode(br#"{"keys":{"key.chat":0}}"#)
            .unwrap()
            .named_key_control(BindingId::Chat),
        None
    );
    let default = SettingsOptions::default().named_key_control(BindingId::Chat);
    assert!(default.is_some());
    for bytes in [
        br#"{"keys":{}}"#.as_slice(),
        br#"{"keys":{"key.chat":65535}}"#.as_slice(),
    ] {
        assert_eq!(
            SettingsOptions::decode(bytes)
                .unwrap()
                .named_key_control(BindingId::Chat),
            default
        );
    }
}

#[test]
fn unbinding_does_not_enable_an_unrelated_suppressed_default() {
    let mut settings = SettingsOptions::decode(br#"{"keys":{"key.drop":17}}"#).unwrap();
    assert_eq!(
        settings.named_key_control(BindingId::InteractWithToast),
        None
    );
    let original = settings.controls().unwrap();
    let row = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.drop")
            .unwrap();
    assert!(settings.unbind_key(row));
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(loaded.named_key_control(BindingId::Drop), None);
    assert_eq!(loaded.named_key_control(BindingId::InteractWithToast), None);
    assert_eq!(loaded.controls().unwrap(), original);
}

#[test]
fn clearing_keyboard_actions_keeps_their_touch_controls_working_after_reload() {
    let defaults = ControlSettings::default();
    for (row, (action, _)) in KEY_BINDINGS.iter().enumerate() {
        let Some(hit_id) = defaults.bindings().iter().find_map(|binding| {
            if binding.action == *action
                && let PhysicalControl::TouchControl(hit_id) = binding.chord.control
            {
                Some(hit_id)
            } else {
                None
            }
        }) else {
            continue;
        };
        let mut settings = SettingsOptions::default();
        assert!(settings.unbind_key(row));
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        let mut router = semantic_input::SemanticInputRouter::default();
        router.replace_bindings(loaded.controls().unwrap()).unwrap();
        router
            .route(semantic_input::DeviceFrame {
                touches: vec![semantic_input::TouchContact {
                    contact_id: 1,
                    activity_sequence: 1,
                    position: [0.0; 2],
                    delta: [0.0; 2],
                    hit_id: Some(hit_id),
                }],
                ..Default::default()
            })
            .unwrap();
        let phase = router.finalize().unwrap().phases[*action as usize];
        assert!(phase.pressed || phase.held, "{action:?}");
    }
}
