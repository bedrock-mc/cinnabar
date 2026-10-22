//! Builds the launcher presentation from the active menu and account state.

use {super::*, launcher::menu::auth::AuthState, launcher::menu::view::MenuView};

impl MenuRuntime {
    /// Presents current interaction state while retaining each owner's published list storage.
    pub fn view(&self) -> MenuView {
        #[cfg(feature = "developer-control")]
        if let Some(view) = self.fixture_view() {
            return view;
        }
        let auth_state = self.current_auth().into_owned();
        let catalog_loading = self.external.is_none()
            && (matches!(
                &auth_state,
                AuthState::Checking
                    | AuthState::AwaitingCode { .. }
                    | AuthState::AwaitingXboxSignup { .. }
            ) || (auth_state == AuthState::Authenticated
                && (!self.catalog_started || self.catalog.is_running())));
        let auth_state = if self.presentation_accounts {
            AuthState::Authenticated
        } else {
            auth_state
        };
        MenuView {
            updates: launcher_host::lifecycle::update::view(),
            visible: self.visible,
            over_world: self.over_world(),
            screen: self.screen,
            focused_action: self.focus_actions().get(self.focused).copied(),
            hovered: self.hovered,
            pressed: self.pressed,
            navigation_focus_visible: self.input_mode.navigation(),
            gamepad_input: self.input_mode.gamepad(),
            touch_available: self.touch_available,
            server_tab: self.server_tab,
            profile_tab: self.profile_tab,
            dialog: self.dialog,
            field: self.field,
            friend_query: self.friend_query.as_str().to_owned(),
            settings_query: self.settings_query.as_str().to_owned(),
            caret: self.caret(),
            name: self.name.as_str().to_owned(),
            address: self.address.as_str().to_owned(),
            port: self.port.as_str().to_owned(),
            macro_commands: (self.screen == launcher::menu::MenuScreen::Settings).then(|| {
                core::array::from_fn(|slot| self.macro_commands[slot].as_str().to_owned())
            }),
            message: self.message.clone(),
            death_reason: String::new(),
            death_loading: self.death_loading,
            death_presentation: self.death_presentation,
            death_controls_visible: self.death_controls_ready(),
            gui_scale_offset: self.gui_scale_display_offset,
            gui_scale_choices: self.gui_scale_choices.clone(),
            fullscreen: self.fullscreen,
            render_mode: self.render_mode,
            shader_packs: std::sync::Arc::clone(&self.shader_packs),
            shader_packs_revision: self.shader_packs_revision,
            shader_pack_removing: self.shader_pack_removing.clone(),
            shader_pack: (self.render_mode == ui::RenderMode::ShaderPack)
                .then(|| self.shader_pack.clone())
                .flatten(),
            renderer: self.renderer_choice(),
            shader_options_available: self.shader_options_available(),
            renderer_failure: self.renderer_failure(),
            shader_pack_loading: self.renderer_loading(),
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
            catalog_message: if self.external.is_some() {
                Some(external::SERVICE_JOINS_UNAVAILABLE.into())
            } else {
                self.catalog_message.clone()
            },
            sign_in_browser: self.sign_in_browser.state(&auth_state),
            sign_in_requested: self.sign_in_requested,
            auth_state,
            connecting: self.is_connecting(),
            dressing_room: self.dressing_room.clone(),
            player_skin: Some(self.player_skin.standard_skin()),
            player_skin_model: self.player_skin.model(),
            disconnect_message: self.disconnect_message.clone(),
            can_reconnect: self.can_reconnect(),
            editing: self.editing,
            local_worlds: self.local_worlds.clone(),
            local: self.local_view(),
            settings: self.settings,
            settings_options: std::sync::Arc::clone(&self.settings_options),
            storage: std::sync::Arc::clone(&self.storage),
            language_choices: std::sync::Arc::clone(&self.language_choices),
            feeds: self.presented_feeds(),
            hosting: self.hosting_world(),
            invite: self.invite_view(),
            realm_membership: self.realm_membership.state.clone(),
            join_request: self.join_request_view(),
            global_resources: self.global_resources.clone(),
            mods: self.mods.clone(),
            shader_options: self.shader_options_view(),
            minecraft_import: self.minecraft_import.clone(),
        }
    }
}

impl client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene for MenuRuntime {
    /// Preserves the current menu visibility without a deferred projection.
    fn is_visible(&self) -> bool {
        self.is_visible()
    }
    /// Supplies the same view formerly read directly by UI.
    fn view(&self) -> launcher::menu::MenuView {
        self.view()
    }
    /// Supplies the current visible screen without changing its transition.
    fn scene(&self) -> Option<launcher::menu::MenuScreen> {
        self.scene()
    }
    /// Preserves the menu's relationship to the connected world.
    fn over_world(&self) -> bool {
        self.over_world()
    }
    /// Preserves the launcher's panorama policy.
    fn uses_panorama(&self) -> bool {
        self.uses_panorama()
    }
}

#[cfg(test)]
mod tests;
