//! Platform text input follows the visible menu's focused editor.

use crate::{
    menu::MenuRuntime,
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use bevy::{
    ecs::message::{MessageCursor, Messages},
    prelude::{Local, Query, Res, ResMut, With},
    window::{Ime, PrimaryWindow, Window},
};

/// Requests the platform keyboard for menu fields and delivers committed composition once.
#[allow(
    clippy::too_many_arguments,
    reason = "Window text ownership follows menu, screen and platform input authority."
)]
pub fn drive_menu_ime(
    mut menu: ResMut<MenuRuntime>,
    mut windows: Query<(bevy::prelude::Entity, &mut Window), With<PrimaryWindow>>,
    presentation: Option<Res<UiPresentationResource>>,
    runtime: Option<Res<UiRuntimeResource>>,
    focus: Option<Res<client_state::CursorFocus>>,
    consent: Option<Res<client_state::ConsentInput>>,
    messages: Option<Res<Messages<Ime>>>,
    mut cursor: Local<MessageCursor<Ime>>,
    mut owned: Local<bool>,
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
    let enabled = !mod_owned
        && menu.is_visible()
        && !menu.is_connecting()
        && menu.field.is_some()
        && window.focused
        && focus.as_ref().is_none_or(|focus| focus.available())
        && !consent.as_ref().is_some_and(|consent| consent.0)
        && !runtime.as_ref().is_some_and(|runtime| {
            runtime.server_forms().owns_input() || runtime.credits().owns_input()
        });
    let mod_editor = presentation.as_ref().is_some_and(|presentation| {
        presentation.mod_ui().mod_panel_field_focused() || presentation.mod_ui().mod_text_focused()
    });
    if !mod_editor && (enabled || *owned) {
        if window.ime_enabled != enabled {
            window.ime_enabled = enabled;
        }
        if enabled
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
    if enabled
        && (!*owned || *activation != menu.field_activation)
        && let Some(requests) = requests.as_deref_mut()
    {
        requests.show(entity);
    }
    *activation = menu.field_activation;
    *owned = enabled;
    if let Some(messages) = messages.as_deref() {
        for event in cursor.read(messages) {
            if enabled
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
            .add_systems(Update, drive_menu_ime);
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
            .add_systems(Update, drive_menu_ime);
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
}
