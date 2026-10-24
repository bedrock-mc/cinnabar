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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOwner {
    Menu,
    Chat,
}

/// Whether focused chat requests the platform keyboard. Android's activity delivers on-screen
/// keyboard text as key events, which chat already types; desktop chat leaves IME composition
/// off and takes hardware key text.
pub const CHAT_PLATFORM_KEYBOARD: bool = cfg!(target_os = "android");

/// Picks the editor that owns platform text: a visible menu's focused field, else focused chat
/// where chat requests the platform keyboard.
fn text_owner(
    available: bool,
    menu: &MenuRuntime,
    chat_focused: bool,
    chat_keyboard: bool,
) -> Option<TextOwner> {
    if !available {
        None
    } else if menu.is_visible() {
        (!menu.is_connecting() && menu.field.is_some()).then_some(TextOwner::Menu)
    } else {
        (chat_keyboard && chat_focused).then_some(TextOwner::Chat)
    }
}

/// Requests the platform keyboard for menu fields and Android chat, and delivers menu
/// composition once.
#[allow(
    clippy::too_many_arguments,
    reason = "Window text ownership follows menu, screen and platform input authority."
)]
pub fn drive_window_ime(
    mut menu: ResMut<MenuRuntime>,
    mut windows: Query<(bevy::prelude::Entity, &mut Window), With<PrimaryWindow>>,
    presentation: Option<Res<UiPresentationResource>>,
    runtime: Option<Res<UiRuntimeResource>>,
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
            runtime.server_forms().owns_input()
                || runtime.credits().owns_input()
                || runtime.data_driven_ui().owns_input()
        });
    let owner = text_owner(
        available,
        &menu,
        runtime
            .as_ref()
            .is_some_and(|runtime| runtime.chat_focused()),
        CHAT_PLATFORM_KEYBOARD,
    );
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
            if owner == Some(TextOwner::Menu)
                && let Ime::Commit { window, value } = event
                && *window == entity
            {
                menu.edit_text(value);
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

    /// Chat owns platform text only where it requests the keyboard, and never over a menu.
    #[test]
    fn chat_owns_platform_text_only_where_it_requests_the_keyboard() {
        let mut menu = MenuRuntime::new(false, 2, "Player".into());
        assert_eq!(text_owner(true, &menu, true, true), Some(TextOwner::Chat));
        assert_eq!(text_owner(true, &menu, true, false), None);
        assert_eq!(text_owner(false, &menu, true, true), None);
        assert_eq!(text_owner(true, &menu, false, true), None);
        menu.show_home();
        assert_eq!(text_owner(true, &menu, true, true), None);
    }

    /// Opening chat in game requests the platform keyboard where chat uses it, and closing chat
    /// releases platform text input.
    #[test]
    fn gameplay_chat_owns_platform_keyboard_while_focused() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
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
        assert_eq!(
            app.world().get::<Window>(window).unwrap().ime_enabled,
            CHAT_PLATFORM_KEYBOARD
        );
        assert_eq!(
            app.world()
                .resource::<crate::platform_text::KeyboardRequests>()
                .requested(window),
            CHAT_PLATFORM_KEYBOARD
        );
        app.world_mut()
            .resource_mut::<UiRuntimeResource>()
            .close_chat();
        app.update();
        assert!(!app.world().get::<Window>(window).unwrap().ime_enabled);
    }
}
