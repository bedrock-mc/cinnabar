//! Platform text input follows the visible menu's focused editor or the in-game chat.

use crate::{
    menu::MenuRuntime,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use bevy::{
    ecs::message::{MessageCursor, Messages},
    prelude::{Local, Query, Res, ResMut, With},
    window::{Ime, PrimaryWindow, Window},
};

/// The editor that owns the window's platform text input this frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextOwner {
    Menu,
    Chat,
}

/// Requests the platform keyboard for menu fields and chat and delivers committed composition once.
#[allow(
    clippy::too_many_arguments,
    reason = "Window text ownership follows menu, screen and platform input authority."
)]
pub fn drive_window_ime(
    mut menu: ResMut<MenuRuntime>,
    mut windows: Query<(bevy::prelude::Entity, &mut Window), With<PrimaryWindow>>,
    presentation: Option<Res<UiPresentationResource>>,
    mut runtime: Option<ResMut<UiRuntimeResource>>,
    focus: Option<Res<client_state::CursorFocus>>,
    consent: Option<Res<client_state::ConsentInput>>,
    messages: Option<Res<Messages<Ime>>>,
    mut cursor: Local<MessageCursor<Ime>>,
    mut owned: Local<Option<TextOwner>>,
    mut activation: Local<u64>,
    mut requests: Option<ResMut<crate::platform_text::KeyboardRequests>>,
) {
    let Ok((entity, mut window)) = windows.single_mut() else {
        return;
    };
    let mod_owned = presentation.as_ref().is_some_and(|presentation| {
        presentation.mod_ui().mod_panel_open()
            || presentation.mod_ui().mod_view_shown()
            || presentation.mod_ui().experience_modal_shown()
    });
    let available = !mod_owned
        && window.focused
        && focus.as_ref().is_none_or(|focus| focus.available())
        && !consent.as_ref().is_some_and(|consent| consent.0)
        && !runtime.as_ref().is_some_and(|runtime| {
            runtime.server_forms().owns_input() || runtime.credits().owns_input()
        });
    let owner = if !available {
        None
    } else if menu.is_visible() {
        (!menu.is_connecting() && menu.field.is_some()).then_some(TextOwner::Menu)
    } else {
        runtime
            .as_ref()
            .is_some_and(|runtime| runtime.chat_focused())
            .then_some(TextOwner::Chat)
    };
    let enabled = owner.is_some();
    let mod_editor = presentation.as_ref().is_some_and(|presentation| {
        presentation.mod_ui().mod_panel_field_focused() || presentation.mod_ui().mod_text_focused()
    });
    if !mod_editor && (enabled || owned.is_some()) {
        if window.ime_enabled != enabled {
            window.ime_enabled = enabled;
        }
        if owner == Some(TextOwner::Menu)
            && let Some(position) = menu.field.and_then(|field| {
                presentation
                    .as_ref()
                    .and_then(|presentation| presentation.menu_ime_position(field))
            })
        {
            let position = bevy::prelude::Vec2::from_array(position);
            if window.ime_position != position {
                window.ime_position = position;
            }
        }
    }
    let retapped = owner == Some(TextOwner::Menu) && *activation != menu.field_activation;
    if enabled
        && (*owned != owner || retapped)
        && let Some(requests) = requests.as_deref_mut()
    {
        requests.show(entity);
    }
    *activation = menu.field_activation;
    *owned = owner;
    if let Some(messages) = messages.as_deref() {
        for event in cursor.read(messages) {
            let Ime::Commit { window, value } = event else {
                continue;
            };
            if *window != entity {
                continue;
            }
            match owner {
                Some(TextOwner::Menu) => menu.edit_text(value),
                Some(TextOwner::Chat) => {
                    if let Some(runtime) = runtime.as_deref_mut() {
                        let _ = runtime.insert_chat_text(value);
                    }
                }
                None => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use launcher::menu::{MenuAction, MenuField, MenuScreen};

    /// Back dismisses the OS keyboard without changing focus; a new field or retap requests it.
    #[test]
    fn server_name_back_address_and_retap_request_platform_keyboard() {
        let mut app = App::new();
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.activate(MenuAction::Navigate(MenuScreen::AddServer));
        menu.focus_field(MenuField::Name);
        app.add_plugins(MinimalPlugins)
            .init_resource::<crate::platform_text::KeyboardRequests>()
            .insert_resource(menu)
            .add_systems(Update, drive_window_ime);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        for field in [MenuField::Name, MenuField::Address, MenuField::Address] {
            app.world_mut()
                .resource_mut::<MenuRuntime>()
                .focus_field(field);
            app.update();
            assert!(
                app.world()
                    .resource::<crate::platform_text::KeyboardRequests>()
                    .requested(window)
            );
            // The platform hides its keyboard while Window.ime_enabled remains true.
            app.world_mut()
                .insert_resource(crate::platform_text::KeyboardRequests::default());
            app.update();
            assert!(
                !app.world()
                    .resource::<crate::platform_text::KeyboardRequests>()
                    .requested(window)
            );
            assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
        }
    }

    /// Menu focus requests a keyboard, commits Unicode, and releases it with the scene or window.
    #[test]
    fn friends_search_owns_platform_text_only_while_focused() {
        let mut app = App::new();
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.activate(MenuAction::Navigate(MenuScreen::Friends));
        menu.activate(MenuAction::SearchFriends);
        app.add_plugins(MinimalPlugins)
            .add_message::<Ime>()
            .insert_resource(menu)
            .add_systems(Update, drive_window_ime);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
        app.world_mut().write_message(Ime::Commit {
            window,
            value: "Player 日本".into(),
        });
        app.update();
        assert_eq!(
            app.world()
                .resource::<MenuRuntime>()
                .field_text(MenuField::FriendQuery),
            "Player 日本"
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<MenuRuntime>()
                .field_text(MenuField::FriendQuery),
            "Player 日本"
        );
        app.insert_resource(client_state::ConsentInput(true));
        app.world_mut().write_message(Ime::Commit {
            window,
            value: "blocked".into(),
        });
        app.update();
        assert!(!app.world().get::<Window>(window).unwrap().ime_enabled);
        assert_eq!(
            app.world()
                .resource::<MenuRuntime>()
                .field_text(MenuField::FriendQuery),
            "Player 日本"
        );
        app.world_mut()
            .remove_resource::<client_state::ConsentInput>();
        app.update();
        assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.world_mut().write_message(Ime::Commit {
            window,
            value: "discarded".into(),
        });
        app.update();
        assert!(!app.world().get::<Window>(window).unwrap().ime_enabled);
        assert_eq!(
            app.world()
                .resource::<MenuRuntime>()
                .field_text(MenuField::FriendQuery),
            "Player 日本"
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
        app.world_mut().resource_mut::<MenuRuntime>().show_home();
        app.update();
        assert!(!app.world().get::<Window>(window).unwrap().ime_enabled);
    }

    /// Opening chat in game requests the keyboard, commits Unicode into the draft, and closing
    /// chat releases platform text input.
    #[test]
    fn gameplay_chat_owns_platform_keyboard_while_focused() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<Ime>()
            .init_resource::<crate::platform_text::KeyboardRequests>()
            .insert_resource(MenuRuntime::new(false, 2, "Player".into()))
            .insert_resource(UiRuntimeResource(client_ui::ui_runtime::UiRuntime::new(1)))
            .add_systems(Update, drive_window_ime);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        assert!(!app.world().get::<Window>(window).unwrap().ime_enabled);
        let mut player = player_state::PlayerState::new(7);
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .open_chat(&mut player);
        app.update();
        assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
        assert!(
            app.world()
                .resource::<crate::platform_text::KeyboardRequests>()
                .requested(window)
        );
        app.world_mut().write_message(Ime::Commit {
            window,
            value: "hi 日本".into(),
        });
        app.update();
        assert_eq!(
            app.world()
                .resource::<UiRuntimeResource>()
                .chat_editor()
                .as_str(),
            "hi 日本"
        );
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .close_chat();
        app.update();
        assert!(!app.world().get::<Window>(window).unwrap().ime_enabled);
    }
}
