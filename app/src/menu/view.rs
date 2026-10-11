//! Builds the launcher presentation from the active menu and account state.

use {super::*, launcher::menu::auth::AuthState, launcher::menu::view::MenuView};

impl MenuRuntime {
    /// Presents the visible route, modal focus, and current service feeds together.
    pub(crate) fn view(&self) -> MenuView {
        #[cfg(feature = "developer-control")]
        if let Some(view) = self.fixture_view() {
            return view;
        }
        let auth_state = self.current_auth().into_owned();
        let catalog_loading = matches!(
            &auth_state,
            AuthState::Checking
                | AuthState::AwaitingCode { .. }
                | AuthState::AwaitingXboxSignup { .. }
        ) || (auth_state == AuthState::Authenticated
            && (!self.catalog_started || self.catalog.is_running()));
        let auth_state = if self.presentation_accounts {
            AuthState::Authenticated
        } else {
            auth_state
        };
        MenuView {
            visible: self.visible,
            over_world: self.over_world(),
            screen: self.screen,
            focused_action: self.focus_actions().get(self.focused).copied(),
            hovered: self.hovered,
            pressed: self.pressed,
            navigation_focus_visible: self.input_mode.navigation(),
            gamepad_input: self.input_mode.gamepad(),
            server_tab: self.server_tab,
            profile_tab: self.profile_tab,
            dialog: self.dialog,
            field: self.field,
            caret: self.caret(),
            name: self.name.as_str().to_owned(),
            address: self.address.as_str().to_owned(),
            port: self.port.as_str().to_owned(),
            message: self.message.clone(),
            death_reason: String::new(),
            death_loading: self.death_loading,
            death_presentation: self.death_presentation,
            death_controls_visible: self.death_controls_ready(),
            gui_scale_offset: self.gui_scale_display_offset,
            gui_scale_choices: self.gui_scale_choices.clone(),
            fullscreen: self.fullscreen,
            render_mode: self.render_mode,
            vsync_override: self.vsync_override,
            display_name: self.presented_display_name(),
            servers: self.servers.clone(),
            featured: self.featured.clone(),
            realms: self.realms.clone(),
            friends: self.friends.clone(),
            featured_icon: None,
            realm_icon: None,
            friend_icon: None,
            saved_icon: None,
            profile_icon: None,
            catalog_loading,
            catalog_message: self.catalog_message.clone(),
            sign_in_browser: self.sign_in_browser.state(&auth_state),
            sign_in_requested: self.sign_in_requested,
            auth_state,
            connecting: self.is_connecting(),
            settings_section: self.settings_section,
            dressing_room: self.dressing_room.clone(),
            player_skin: Some(self.player_skin.standard_skin()),
            player_skin_model: self.player_skin.model(),
            disconnect_message: self.disconnect_message.clone(),
            can_reconnect: self.can_reconnect(),
            editing: self.editing,
            local_worlds: self.local_worlds.clone(),
            local: self.local_view(),
            settings_options: std::sync::Arc::clone(&self.settings_options),
            storage: std::sync::Arc::clone(&self.storage),
            settings_dropdown: self.settings_dropdown,
            settings_scale_picker: self.settings_scale_picker,
            settings_control_activation: self.settings_control_activation,
            settings_control_activation_navigation: self.settings_control_activation_navigation,
            settings_slider_pointer: self.settings_slider_pointer,
            settings_slider_hovered: self.settings_slider_hovered,
            settings_slider_selected: self.settings_slider_selected,
            language_choices: std::sync::Arc::clone(&self.language_choices),
            key_remap: self.key_remap,
            settings_advanced_graphics: self.settings_advanced_graphics,
            feeds: self.presented_feeds(),
            store: self.store_snapshot.clone(),
            hosting: self.hosting_world(),
            invite: self.invite_view(),
            realm_membership: self.realm_membership.state.clone(),
            join_request: self.join_request_view(),
            global_resources: self.global_resources.clone(),
        }
    }
}
