use super::*;
use crate::semantic_controls::ReservedControls;
use std::sync::Arc;

/// Publishes a saved shortcut and its physical reservation before guest key capture begins.
fn capturing_shortcut_fixture() -> String {
    let panel = r#"{"title":"Shortcut capture","toggle_key":"ShiftRight","dark":true,"capture_key":true,"controls":[{"kind":"keybind","id":"shortcut","label":"Shortcut","key":"KeyR","capturing":true}]}"#;
    fixture_with_panel(panel)
        .replace(
            "(export \"demo-pressed\" (func (result bool)))",
            "(export \"demo-pressed\" (func (result bool))) (export \"reserve-keys\" (func (param \"keys\" (list string)) (result (result (error string)))))",
        )
        .replace(
            "(alias export $input \"demo-pressed\" (func $demo-pressed))",
            "(alias export $input \"demo-pressed\" (func $demo-pressed)) (alias export $input \"reserve-keys\" (func $reserve-keys))",
        )
        .replace(
            "(core func $lower-input",
            "(core func $lower-reserve (canon lower (func $reserve-keys) (memory $memory) (realloc $realloc))) (core func $lower-input",
        )
        .replace(
            "(import \"host\" \"pressed\"",
            "(import \"host\" \"reserve\" (func $reserve (param i32 i32 i32))) (import \"host\" \"pressed\"",
        )
        .replace(
            "(export \"pressed\" (func $lower-input))",
            "(export \"pressed\" (func $lower-input)) (export \"reserve\" (func $lower-reserve))",
        )
        .replace(
            "(data (i32.const 0)",
            "(data (i32.const 2048) \"KeyR\") (data (i32.const 2080) \"\\00\\08\\00\\00\\04\\00\\00\\00\") (data (i32.const 0)",
        )
        .replacen(
            "(func (export \"init\")",
            "(func (export \"init\") i32.const 2080 i32.const 1 i32.const 512 call $reserve",
            1,
        )
}

/// Reserved menu keys reach guest conflict validation without closing its already-open capture.
#[test]
fn personal_panel_reserved_shortcuts_reach_capture_without_toggling_or_gameplay_replay() {
    for (key_code, logical_key) in [
        (KeyCode::ShiftRight, Key::Shift),
        (KeyCode::F10, Key::F10),
        (KeyCode::Escape, Key::Escape),
    ] {
        let component = capturing_shortcut_fixture();
        let (mut app, window) = personal_panel_app_with_fixture(false, &component);
        app.init_resource::<ReservedControls>();
        app.update();
        let runtime = app.world().resource::<ModRuntime>();
        let saved = runtime.hosts.host(0).panel().unwrap().clone();
        let reservations = runtime.hosts.host(0).reserved_keys().to_vec();
        assert_eq!(reservations, ["KeyR"]);
        assert!(
            matches!(&saved.controls[0], ui::mod_panel::Control::Keybind { key, .. } if key == "KeyR")
        );
        let gameplay_reservations = app.world().resource::<ReservedControls>().0.clone();
        let event = KeyboardInput {
            key_code,
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        };
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key_code);
        app.world_mut().write_message(event.clone());
        app.update();
        let runtime = app.world().resource::<ModRuntime>();
        assert!(
            runtime.hosts.host(0).panel_open(),
            "{key_code:?} closed guest capture"
        );
        assert!(
            app.world()
                .resource::<UiPresentationResource>()
                .mod_ui()
                .mod_panel_open()
        );
        assert!(
            runtime.controls.panel_open && runtime.controls.focused && !runtime.controls.gameplay
        );
        assert_eq!(runtime.controls.keys_pressed, [format!("{key_code:?}")]);
        assert!(
            runtime
                .controls
                .keys_held
                .contains(&format!("{key_code:?}"))
        );
        assert!(runtime.controls.events.is_empty());
        assert_eq!(runtime.hosts.host(0).panel(), Some(&saved));
        assert_eq!(runtime.hosts.host(0).reserved_keys(), reservations);
        assert_eq!(
            app.world().resource::<ReservedControls>().0,
            gameplay_reservations
        );
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.just_pressed(key_code) && !keys.just_released(key_code));

        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key_code);
        app.world_mut().write_message(KeyboardInput {
            state: ButtonState::Released,
            ..event.clone()
        });
        app.update();
        assert!(
            app.world()
                .resource::<ModRuntime>()
                .controls
                .keys_pressed
                .is_empty()
        );
        assert!(
            app.world()
                .resource::<ModRuntime>()
                .hosts
                .host(0)
                .panel_open()
        );

        // End capture through normal publication, retaining the saved shortcut and reservation.
        let mut normal = saved;
        normal.capture_key = false;
        let json = serde_json::json!({"panel":normal,"surfaces":{}}).to_string();
        app.world_mut()
            .resource_mut::<ModRuntime>()
            .hosts
            .host_mut(0)
            .set_panel_bundle(&json)
            .unwrap();
        app.world_mut()
            .resource_mut::<UiPresentationResource>()
            .mod_ui_mut()
            .set_mod_panel(PanelUiOwner::Environment(0), Some(&normal))
            .unwrap();
        let normal_open_states: &[bool] = if key_code == KeyCode::Escape {
            &[false]
        } else {
            &[false, true]
        };
        for &expected_open in normal_open_states {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key_code);
            app.world_mut().write_message(event.clone());
            app.update();
            let runtime = app.world().resource::<ModRuntime>();
            assert_eq!(runtime.hosts.host(0).panel_open(), expected_open);
            assert_eq!(runtime.controls.panel_open, expected_open);
            assert_eq!(
                app.world()
                    .resource::<UiPresentationResource>()
                    .mod_ui()
                    .mod_panel_open(),
                expected_open
            );
            assert_eq!(runtime.hosts.host(0).panel(), Some(&normal));
            assert_eq!(runtime.hosts.host(0).reserved_keys(), reservations);
            let keys = app.world().resource::<ButtonInput<KeyCode>>();
            assert!(
                !keys.just_pressed(key_code),
                "toggle leaked a gameplay edge"
            );
            if !expected_open {
                assert!(!keys.pressed(key_code), "closing toggle leaked to gameplay");
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(key_code);
            app.world_mut().write_message(KeyboardInput {
                state: ButtonState::Released,
                ..event.clone()
            });
            app.update();
            assert_eq!(
                app.world()
                    .resource::<ModRuntime>()
                    .hosts
                    .host(0)
                    .panel_open(),
                expected_open
            );
            assert!(
                app.world()
                    .resource::<ModRuntime>()
                    .controls
                    .keys_pressed
                    .is_empty()
            );
        }
    }
}

/// Idle input updates keep the capturing panel's catalog and laid-out screen cached.
#[test]
fn personal_panel_idle_capture_reuses_catalog_and_layout() {
    let (mut app, _) = personal_panel_app_with_fixture(false, &capturing_shortcut_fixture());
    let mut retained = None;
    for _ in 0..3 {
        app.update();
        app.world_mut()
            .resource_scope(|world, mut presentation: Mut<UiPresentationResource>| {
                render(
                    &mut presentation,
                    world.resource::<client_state::PlayerRuntime>(),
                    &world.resource::<UiRuntimeResource>().0,
                );
                let state = presentation.mod_ui().mod_panel_state().unwrap();
                let catalog = state.catalog.unwrap();
                if let Some((original, passes)) = &retained {
                    assert!(Arc::ptr_eq(original, catalog));
                    assert_eq!(*passes, state.passes);
                } else {
                    retained = Some((Arc::clone(catalog), state.passes));
                }
            });
    }
}
