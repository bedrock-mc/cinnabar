use super::*;
use crate::{app::ClientFrameSet, player_runtime::PlayerRuntime};
use client_ui::ui_runtime::{SequencedUiEvent, UiRuntime};
use std::sync::Arc;

#[derive(Resource, Clone, Copy)]
enum KeyboardOwner {
    Chat,
    Form,
}

/// Opens a real keyboard-owning screen during the shared UI authority step.
fn open_keyboard_owner(
    owner: Res<KeyboardOwner>,
    mut ui: ResMut<UiRuntime>,
    mut player: ResMut<PlayerRuntime>,
) {
    match *owner {
        KeyboardOwner::Chat => {
            ui.open_chat(&mut player);
        }
        KeyboardOwner::Form => {
            ui.apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: 1,
                    local_millis: 0,
                    server_tick: None,
                    event: protocol::UiEvent::Form(protocol::FormRequestEvent {
                        form_id: 7,
                        kind: protocol::FormKind::Menu,
                        title: Some(Arc::from("Choose")),
                        json: Arc::from("{}"),
                        model: protocol::ServerFormModel::TextMenu(protocol::TextMenuForm {
                            title: "Choose".into(),
                            content: "Pick one".into(),
                            buttons: vec!["First".into()].into(),
                            button_images: [].into(),
                            omitted_images: 0,
                        }),
                    }),
                },
            )
            .unwrap();
        }
    }
}

#[test]
fn enhanced_shortcuts_respect_chat_and_form_keyboard_ownership() {
    for owner in [KeyboardOwner::Chat, KeyboardOwner::Form] {
        let mut app = App::new();
        app.init_resource::<Assets<bevy::shader::Shader>>()
            .init_resource::<RuntimeSettings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(PlayerRuntime::new(1))
            .insert_resource(UiRuntime::new(1))
            .insert_resource(owner)
            .add_plugins(RenderModePlugin::new(Some(RenderMode::Enhanced), false))
            .insert_resource(render::EnhancedRenderSupport(true))
            .add_systems(
                Update,
                open_keyboard_owner.in_set(ClientFrameSet::UiAuthority),
            );
        let camera = app
            .world_mut()
            .spawn((
                FlyCamera::default(),
                Camera3d::default(),
                EnhancedRendering::default(),
            ))
            .id();
        app.world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(ENHANCED_SHADOW_KEY);
        keys.press(ENHANCED_TIME_KEY);
        app.update();
        assert_eq!(
            app.world()
                .get::<EnhancedRendering>(camera)
                .unwrap()
                .shadow_debug,
            EnhancedShadowDebug::Off,
            "typing into an owned screen must not change shadow diagnostics",
        );
        assert_eq!(
            app.world().resource::<DebugTimeOverride>().ticks,
            None,
            "typing into an owned screen must not freeze the visual clock",
        );
    }
}
