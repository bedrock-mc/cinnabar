use super::*;
use crate::{GameplaySnapshot, GameplayVector3, ModGrants};
use cinnabar::extension::{input::Host as _, panel::Host as _, settings::Host as _};

fn state(grants: ModGrants) -> State {
    State::new(grants, "{\"cps\":12}".into())
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
