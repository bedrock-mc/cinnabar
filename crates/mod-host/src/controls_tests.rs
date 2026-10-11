use super::*;
use crate::{
    GameplaySnapshot, GameplayVector3, ModGrants,
    runtime::{CALLBACK_FUEL, FRAME_FUEL, Instance, commit_event},
};
use cinnabar::extension::{
    input::{Host as _, Selection},
    panel::Host as _,
    settings::Host as _,
};
use wasmtime::Engine;

fn state(grants: ModGrants) -> State {
    State::new(grants, "{\"cps\":12}".into(), Default::default())
}

fn panel() -> String {
    r#"{"title":"Local controls","toggle_key":"ShiftRight","dark":true,"controls":[{"kind":"toggle","id":"enabled","label":"Enabled","value":false}]}"#.into()
}

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        yaw: 0.0,
        pitch: 0.0,
        frame_seconds: 0.01,
        attack_held: true,
        players: Vec::new(),
    }
}

#[test]
fn optional_imports_deny_without_explicit_grants() {
    let mut state = state(ModGrants::default());
    assert!(state.read_controls().unwrap().is_err());
    assert!(
        state
            .read_selected_controls(Selection {
                keys_pressed: None,
                keys_held: None,
                events: true,
            })
            .unwrap()
            .is_err()
    );
    assert!(state.reserve_keys(vec!["KeyA".into()]).unwrap().is_err());
    assert!(state.set_content(panel()).unwrap().is_err());
    assert!(state.load().unwrap().is_err());
    assert!(state.save("{}".into()).unwrap().is_err());
    assert!(state.set_reach(Some(5.0)).unwrap().is_err());
    assert!(state.pulse_attack().unwrap().is_err());
    state.controls.commit();
    assert!(state.controls.panel.is_none());
    assert!(state.controls.keys.is_empty());
    assert_eq!(state.controls.interaction, InteractionOutput::default());
}

/// Supplies distinguishable keys and event ordering with every current control flag set.
fn control_frame() -> ControlFrame {
    ControlFrame {
        seconds: 0.01,
        focused: true,
        gameplay: true,
        panel_open: true,
        keys_pressed: vec!["ShiftLeft".into(), "KeyB".into(), "KeyA".into()],
        keys_held: vec!["KeyC".into(), "KeyB".into(), "ShiftLeft".into()],
        events: vec![
            crate::ControlEvent {
                id: "enabled".into(),
                value: 1.0,
            },
            crate::ControlEvent {
                id: "size".into(),
                value: 3.0,
            },
        ],
    }
}

#[test]
fn selected_reads_preserve_full_read_equivalence_and_do_not_consume_observations() {
    let mut state = state(ModGrants {
        controls: true,
        ..Default::default()
    });
    state.controls.frame = control_frame();
    let all = state
        .read_selected_controls(Selection {
            keys_pressed: None,
            keys_held: None,
            events: true,
        })
        .unwrap()
        .unwrap();
    assert_eq!(all, state.read_controls().unwrap().unwrap());

    let selected = state
        .read_selected_controls(Selection {
            keys_pressed: Some(vec!["KeyB".into(), "ShiftLeft".into(), "KeyB".into()]),
            keys_held: Some(vec!["ShiftLeft".into(), "KeyC".into(), "KeyZ".into()]),
            events: false,
        })
        .unwrap()
        .unwrap();
    assert_eq!(selected.keys_pressed, ["ShiftLeft", "KeyB"]);
    assert_eq!(selected.keys_held, ["KeyC", "ShiftLeft"]);
    assert!(selected.events.is_empty());
    assert_eq!(
        selected,
        ControlFrame {
            keys_pressed: selected.keys_pressed.clone(),
            keys_held: selected.keys_held.clone(),
            events: Vec::new(),
            ..all.clone()
        }
    );
    assert_eq!(state.read_controls().unwrap().unwrap(), all);

    let empty = state
        .read_selected_controls(Selection {
            keys_pressed: Some(Vec::new()),
            keys_held: Some(Vec::new()),
            events: true,
        })
        .unwrap()
        .unwrap();
    assert!(empty.keys_pressed.is_empty() && empty.keys_held.is_empty());
    assert_eq!(empty.events, all.events);
}

#[test]
fn selected_reads_validate_each_name_list_and_share_the_callback_read_limit() {
    let mut state = state(ModGrants {
        controls: true,
        ..Default::default()
    });
    for names in [
        vec![String::new()],
        vec!["Key-A".into()],
        vec!["x".repeat(mod_api::MAX_CONTROL_KEY_BYTES + 1)],
        vec!["KeyA".into(); mod_api::MAX_CONTROL_KEYS + 1],
    ] {
        for pressed in [true, false] {
            state.controls.begin_frame();
            let selection = Selection {
                keys_pressed: pressed.then(|| names.clone()),
                keys_held: (!pressed).then(|| names.clone()),
                events: false,
            };
            assert!(state.read_selected_controls(selection).unwrap().is_err());
        }
    }
    state.controls.begin_frame();
    state.controls.frame = control_frame();
    let bounded = state
        .read_selected_controls(Selection {
            keys_pressed: Some(vec!["KeyA".into(); mod_api::MAX_CONTROL_KEYS]),
            keys_held: Some(vec!["x".repeat(mod_api::MAX_CONTROL_KEY_BYTES)]),
            events: false,
        })
        .unwrap()
        .unwrap();
    assert_eq!(bounded.keys_pressed, ["KeyA"]);
    assert!(bounded.keys_held.is_empty());
    state.controls.begin_frame();
    state.controls.frame = control_frame();
    for index in 0..MAX_IMPORT_WRITES {
        if index % 2 == 0 {
            state.read_controls().unwrap().unwrap();
        } else {
            state
                .read_selected_controls(Selection {
                    keys_pressed: None,
                    keys_held: Some(Vec::new()),
                    events: false,
                })
                .unwrap()
                .unwrap();
        }
    }
    assert!(
        state
            .read_selected_controls(Selection {
                keys_pressed: None,
                keys_held: None,
                events: true,
            })
            .is_err()
    );
    state.controls.begin_frame();
    let renewed = state
        .read_selected_controls(Selection {
            keys_pressed: None,
            keys_held: None,
            events: true,
        })
        .unwrap()
        .unwrap();
    assert_eq!(renewed, empty_controls());
    assert_eq!(state.read_controls().unwrap().unwrap(), renewed);
}

#[test]
fn controls_publish_after_success_and_revoke_after_trap() {
    let mut state = state(ModGrants {
        controls: true,
        settings: true,
        ..Default::default()
    });
    assert_eq!(state.load().unwrap().unwrap(), "{\"cps\":12}");
    state.set_content(panel()).unwrap().unwrap();
    state
        .reserve_keys(vec!["F8".into(), "F8".into()])
        .unwrap()
        .unwrap();
    state.save("{\"cps\":20}".into()).unwrap().unwrap();
    assert!(state.controls.panel.is_none());
    assert!(state.controls.dirty_settings.is_none());
    state.controls.commit();
    assert!(state.controls.panel.is_some());
    assert_eq!(state.controls.keys, ["F8"]);
    assert_eq!(
        state.controls.dirty_settings.as_deref(),
        Some("{\"cps\":20}")
    );
    state.controls.open = true;
    state.controls.revoke();
    assert!(!state.controls.open);
    assert!(state.controls.panel.is_none());
    assert!(state.controls.keys.is_empty());
}

#[test]
fn interaction_requires_current_held_input_and_never_survives_frame() {
    let mut state = state(ModGrants {
        interaction: true,
        ..Default::default()
    });
    assert!(state.set_reach(Some(5.0)).unwrap().is_err());
    assert!(state.pulse_attack().unwrap().is_err());
    state.snapshot = Some(snapshot());
    for value in [
        f32::NAN,
        f32::INFINITY,
        -1.0,
        mod_api::MAX_ENTITY_REACH_BLOCKS + 1.0,
    ] {
        assert!(state.set_reach(Some(value)).unwrap().is_err());
    }
    state.controls.begin_frame();
    state.set_reach(Some(5.0)).unwrap().unwrap();
    state.pulse_attack().unwrap().unwrap();
    state.pulse_attack().unwrap().unwrap();
    assert_eq!(state.controls.interaction, InteractionOutput::default());
    state.controls.commit();
    assert_eq!(
        state.controls.interaction,
        InteractionOutput {
            attack_reach: Some(5.0),
            attack_pulse: true
        }
    );
    state.controls.begin_frame();
    assert_eq!(state.controls.interaction, InteractionOutput::default());
    state.snapshot.as_mut().unwrap().attack_held = false;
    assert!(state.pulse_attack().unwrap().is_err());
}

#[test]
fn rejects_malformed_controls_and_bounded_import_spam() {
    let mut state = state(ModGrants {
        controls: true,
        settings: true,
        ..Default::default()
    });
    assert!(
        state
            .set_content(panel().replace("\"value\":false", "\"value\":0"))
            .unwrap()
            .is_err()
    );
    assert!(state.reserve_keys(vec!["../file".into()]).unwrap().is_err());
    assert!(state.save("[]".into()).unwrap().is_err());
    assert!(
        state
            .save(format!(
                "{{\"value\":\"{}\"}}",
                "a".repeat(mod_api::MAX_SETTINGS_BYTES)
            ))
            .unwrap()
            .is_err()
    );
    state.controls.begin_frame();
    for _ in 0..MAX_IMPORT_WRITES {
        state.read_controls().unwrap().unwrap();
    }
    assert!(state.read_controls().is_err());
    let mut frame = empty_controls();
    frame.seconds = f32::NAN;
    assert!(validate_frame(&frame).is_err());
    frame.seconds = 0.01;
    frame.events.push(crate::ControlEvent {
        id: "../bad".into(),
        value: 1.0,
    });
    assert!(validate_frame(&frame).is_err());
}

#[test]
fn callback_controls_budgets_are_independent_and_preserve_retained_state() {
    let mut config = wasmtime::Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config).unwrap();
    let component = br#"(component
        (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#;
    let mut instance = Instance::new(
        &engine,
        component,
        ModGrants {
            controls: true,
            settings: true,
            ..Default::default()
        },
        "{}".into(),
        Default::default(),
    )
    .unwrap();
    let state = instance.store.data_mut();
    state.set_content(panel()).unwrap().unwrap();
    state.reserve_keys(vec!["KeyR".into()]).unwrap().unwrap();
    state.controls.commit();
    state.controls.begin_frame();
    state.controls.frame = control_frame();
    for (fuel, settings_reads) in [
        (FRAME_FUEL, false),
        (CALLBACK_FUEL, true),
        (CALLBACK_FUEL, false),
    ] {
        instance
            .run(fuel, commit_event, |_, store| {
                for _ in 0..MAX_IMPORT_WRITES {
                    let state = store.data_mut();
                    if settings_reads {
                        assert_eq!(state.load()?.unwrap(), "{}");
                    } else {
                        assert_eq!(state.read_controls()?.unwrap(), control_frame());
                    }
                    state.save("{}".into())?.unwrap();
                }
                Ok(())
            })
            .unwrap();
        assert!(instance.active);
        let controls = &instance.store.data().controls;
        assert_eq!(controls.frame, control_frame());
        assert!(controls.panel.is_some());
        assert_eq!(controls.keys, ["KeyR"]);
        assert_eq!(controls.settings(), "{}");
    }
    assert!(
        instance
            .run(CALLBACK_FUEL, commit_event, |_, store| {
                for _ in 0..=MAX_IMPORT_WRITES {
                    store.data_mut().read_controls()?.unwrap();
                }
                Ok(())
            })
            .is_err()
    );
    assert!(!instance.active);
}

#[test]
fn larger_panel_catalogs_do_not_increase_the_input_batch_limit() {
    let mut frame = empty_controls();
    frame.events = (0..ui::mod_panel::MAX_PANEL_EVENTS)
        .map(|index| crate::ControlEvent {
            id: format!("setting_{index}"),
            value: 1.0,
        })
        .collect();
    assert!(validate_frame(&frame).is_ok());
    frame.events.push(crate::ControlEvent {
        id: "overflow".into(),
        value: 1.,
    });
    assert!(validate_frame(&frame).is_err());
}
