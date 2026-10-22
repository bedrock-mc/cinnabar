//! A screen's consumed Back edge cannot activate a remapped control afterward.
use super::super::*;
use bevy::prelude::*;
use client_presentation::local_player::{InteractionOriginSnapshot, LocalPlayerFrameCarrier};
use client_state::SemanticInputSnapshot;

#[test]
fn consumed_mouse_back_does_not_reopen_chat_or_inventory_after_bed_or_sign() {
    use bevy::input::mouse::MouseButtonInput;
    use client_ui::ui_runtime::sign_editor::SignEdit;
    use launcher::menu::settings_options::{EXTRA_KEYS, KEY_BINDINGS};
    for sign in [false, true] {
        for binding in [BindingId::Inventory, BindingId::Chat, BindingId::Command] {
            let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
            let mut runtime = client_ui::test_support::inventory_session(&mut player);
            if sign {
                let mut edit = SignEdit::new([0, 64, 0], true, Default::default());
                edit.insert('x', |_| true);
                runtime.sign_editor_mut().open(edit);
            } else {
                runtime.set_local_sleeping(true);
            }
            let mut menu = client_shell::menu::MenuRuntime::new(false, 2, "Bindings".into());
            let row = KEY_BINDINGS
                .iter()
                .position(|(_, name)| *name == binding.name())
                .or_else(|| {
                    EXTRA_KEYS
                        .iter()
                        .position(|(name, _)| *name == binding.name())
                        .map(|row| KEY_BINDINGS.len() + row)
                })
                .unwrap();
            assert!(client_shell::menu::test_support::remap_control(
                &mut menu,
                row,
                semantic_input::PhysicalControl::MouseButton(
                    client_shell::physical_input::mouse_button_code(MouseButton::Back).unwrap()
                )
            ));
            let (network, captured) = client_runtime::network::NetworkHandle::<
                crate::runtime::network::resource_packs::PackApplication,
            >::stub_capturing_packets();
            let mut app = App::new();
            app.add_message::<KeyboardInput>()
                .add_message::<WindowEvent>()
                .init_resource::<Time<Real>>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<AccumulatedMouseMotion>()
                .init_resource::<Touches>()
                .init_resource::<crate::runtime::world::ClientWorld>()
                .init_resource::<InteractionOriginSnapshot>()
                .init_resource::<LocalPlayerFrameCarrier>()
                .init_resource::<SemanticInputSnapshot>()
                .insert_resource(player)
                .insert_resource(menu)
                .insert_resource(UiRuntimeResource(runtime))
                .insert_resource(UiPresentationResource(
                    UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
                ))
                .insert_resource(network)
                .insert_non_send(captured)
                .add_systems(
                    Update,
                    (
                        crate::ui_runtime::sign_editor::drive_sign_editor,
                        drive_chat_ui_actions,
                        drive_chat_keyboard_input,
                    )
                        .chain(),
                );
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
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Back);
            app.world_mut()
                .write_message(WindowEvent::MouseButtonInput(MouseButtonInput {
                    button: MouseButton::Back,
                    state: ButtonState::Pressed,
                    window,
                }));
            app.update();
            let mut runtime = app.world_mut().resource_mut::<UiRuntimeResource>();
            assert!(
                !runtime.inventory_open(),
                "sign={sign}, binding={binding:?}"
            );
            assert!(!runtime.chat_focused(), "sign={sign}, binding={binding:?}");
            assert!(!runtime.sign_editor().is_open());
            if !sign {
                assert!(runtime.flush_wake_request(Some(42), |_| Ok::<_, ()>(())));
            }
        }
    }
}
