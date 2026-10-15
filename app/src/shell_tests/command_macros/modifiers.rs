//! Configurable modifiers use physical event order for keyboard and mouse macros.
use super::*;
use launcher::menu::{
    MenuAction,
    settings_options::{SettingId, macro_modifier::MacroModifier},
};

/// Selects the modifier through the settings action used by the dropdown.
fn select(app: &mut App, modifier: MacroModifier) {
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsOption(
            SettingId::MacroModifier.index() as u16,
            modifier as i32,
        ));
}

#[test]
fn configured_modifier_accepts_either_side_and_none_accepts_plain_presses() {
    for (modifier, keys) in [
        (
            MacroModifier::Control,
            vec![KeyCode::ControlLeft, KeyCode::ControlRight],
        ),
        (
            MacroModifier::Shift,
            vec![KeyCode::ShiftLeft, KeyCode::ShiftRight],
        ),
        (
            MacroModifier::Super,
            vec![KeyCode::SuperLeft, KeyCode::SuperRight],
        ),
        (MacroModifier::None, vec![]),
    ] {
        for held in keys.into_iter().map(Some).chain(std::iter::once(None)) {
            for pointer in [false, true] {
                for ordered in [false, true] {
                    let mut app = crate::tests::gameplay_click::gameplay_app(false);
                    app.add_message::<WindowEvent>();
                    arm(
                        &mut app,
                        if pointer {
                            PhysicalControl::MouseButton(1)
                        } else {
                            PhysicalControl::KeyboardUsage(0x2c)
                        },
                    );
                    select(&mut app, modifier);
                    app.update();
                    if let Some(key_code) = held {
                        key(&mut app, key_code, ButtonState::Pressed, ordered);
                    }
                    if pointer {
                        mouse(&mut app, MouseButton::Left, ordered);
                    } else {
                        key(&mut app, KeyCode::Space, ButtonState::Pressed, ordered);
                    }
                    app.update();
                    let runtime = app.world().resource::<UiRuntimeResource>();
                    assert_eq!(
                        runtime.pending_chat_sends().len(),
                        usize::from(held.is_some() || modifier == MacroModifier::None),
                        "{modifier:?}, {held:?}, pointer={pointer}, ordered={ordered}"
                    );
                    if !runtime.pending_chat_sends().is_empty() {
                        let snapshot = app.world().resource::<SemanticInputSnapshot>();
                        assert!(
                            !snapshot
                                .phase(if pointer {
                                    Action::Attack
                                } else {
                                    Action::Jump
                                })
                                .held
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn configured_modifier_observes_press_and_release_order() {
    for pointer in [false, true] {
        for release in [false, true] {
            for modifier_first in [false, true] {
                let mut app = crate::tests::gameplay_click::gameplay_app(false);
                app.add_message::<WindowEvent>();
                arm(
                    &mut app,
                    if pointer {
                        PhysicalControl::MouseButton(1)
                    } else {
                        PhysicalControl::KeyboardUsage(0x2c)
                    },
                );
                select(&mut app, MacroModifier::Control);
                app.update();
                if release {
                    key(&mut app, KeyCode::ControlRight, ButtonState::Pressed, true);
                    app.update();
                }
                let state = if release {
                    ButtonState::Released
                } else {
                    ButtonState::Pressed
                };
                if modifier_first {
                    key(&mut app, KeyCode::ControlRight, state, true);
                }
                if pointer {
                    mouse(&mut app, MouseButton::Left, true);
                } else {
                    key(&mut app, KeyCode::Space, ButtonState::Pressed, true);
                }
                if !modifier_first {
                    key(&mut app, KeyCode::ControlRight, state, true);
                }
                app.update();
                assert_eq!(
                    app.world()
                        .resource::<UiRuntimeResource>()
                        .pending_chat_sends()
                        .len(),
                    usize::from(modifier_first != release),
                    "pointer={pointer}, release={release}, modifier_first={modifier_first}"
                );
            }
        }
    }
}

#[test]
fn plain_macro_precedes_emote_bindings_but_never_runs_while_chat_owns_input() {
    for pointer in [false, true] {
        for chat in [false, true] {
            let mut app = crate::tests::gameplay_click::gameplay_app(false);
            app.add_message::<WindowEvent>();
            let control = if pointer {
                PhysicalControl::MouseButton(2)
            } else {
                PhysicalControl::KeyboardUsage(
                    client_shell::physical_input::keyboard_usage(KeyCode::KeyB).unwrap(),
                )
            };
            arm(&mut app, control);
            select(&mut app, MacroModifier::None);
            let row = launcher::menu::settings_options::BindingId::Emote
                .row(false)
                .unwrap();
            assert!(menu_fixture::remap_control(
                &mut app.world_mut().resource_mut::<MenuRuntime>(),
                row,
                control,
            ));
            app.update();
            if chat {
                key(&mut app, KeyCode::KeyT, ButtonState::Pressed, true);
                app.update();
                assert!(app.world().resource::<UiRuntimeResource>().chat_focused());
            }
            if pointer {
                mouse(&mut app, MouseButton::Right, true);
            } else {
                key(&mut app, KeyCode::KeyB, ButtonState::Pressed, true);
            }
            app.update();
            let runtime = app.world().resource::<UiRuntimeResource>();
            assert_eq!(runtime.pending_chat_sends().len(), usize::from(!chat));
            assert!(!runtime.emotes().is_open());
        }
    }
}
