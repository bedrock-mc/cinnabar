//! Dispatches menu actions after the current focus and account guards.

use super::{
    DEFAULT_PORT, MenuAction, MenuDialog, MenuField, MenuRuntime, MenuScreen, navigation, now_unix,
    split_address,
};

impl MenuRuntime {
    /// Applies one menu action after checking current prompts and pending account changes.
    pub fn activate(&mut self, action: MenuAction) {
        if self.dismiss_minecraft_import(action) {
            return;
        }
        if self.disconnect_message.is_some() && self.join_request_prompted() {
            if let MenuAction::JoinRequest(accept) = action {
                self.answer_join_request(accept);
            }
            return;
        }
        if action == MenuAction::Reconnect {
            self.reconnect();
            return;
        }
        if self.disconnect_message.is_some()
            && !self.is_connecting()
            && self.dialog.is_none()
            && self.sign_in_focus().is_none()
            && matches!(action, MenuAction::DismissDialog | MenuAction::AddBack)
        {
            self.dismiss_disconnect();
            return;
        }
        #[cfg(feature = "developer-control")]
        if self.activate_sign_in_fixture(action) {
            return;
        }
        if self.skin_editor_blocks(action) || self.presentation_blocks(action) {
            return;
        }
        if self.external_action(action) {
            return;
        }
        if self.account_change_pending()
            && matches!(
                action,
                MenuAction::PlaySaved(_)
                    | MenuAction::PlayFeatured(_)
                    | MenuAction::PlayRealm(_)
                    | MenuAction::PlayFriend(_)
                    | MenuAction::PlayLocalWorld(_)
                    | MenuAction::OpenLiveEvent
                    | MenuAction::LocalWorld(_)
            )
        {
            self.message = Some("Please wait for the account change to finish.".into());
            return;
        }
        if let Some(index) = self
            .focus_actions()
            .iter()
            .position(|candidate| *candidate == action)
        {
            self.focused = index;
        }
        match action.text_field() {
            Some(field) => self.focus_field(field),
            None => self.field = None,
        }
        self.pressed = Some(action);
        self.message = None;
        self.disconnect_message = None;
        match action {
            MenuAction::MinecraftImport(action) => self.activate_minecraft_import(action),
            MenuAction::SelectProfileTab(tab) => self.profile_tab = tab,
            MenuAction::RefreshProfile => {
                self.feeds.profile.loaded = false;
                self.feeds.profile.unavailable = false;
                self.feeds.profile_refresh_requested = true;
            }
            MenuAction::Inbox(action) => self.activate_inbox(action),
            MenuAction::Navigate(screen) => {
                if screen == MenuScreen::DressingRoom {
                    self.ensure_dressing_room();
                }
                self.enter(screen);
            }
            MenuAction::OpenExitDialog => {
                self.dialog = Some(MenuDialog::Exit);
                self.focused = 0;
            }
            MenuAction::Update(action) => {
                if !self.over_world() && launcher_host::lifecycle::update::act(action) {
                    self.intents.exit = true;
                }
            }
            MenuAction::ConfirmExit => {
                self.dialog = None;
                self.intents.exit = true;
            }
            MenuAction::OpenDeathQuit => {
                if self.death_controls_ready() {
                    self.dialog = Some(MenuDialog::DeathQuit);
                    self.focused = 0;
                }
            }
            MenuAction::OpenDeathGameMenu => {
                if self.death_controls_ready() {
                    self.enter(MenuScreen::Pause);
                }
            }
            MenuAction::DeathExitWorld => {
                if self.death_controls_ready() && self.death_presentation.hardcore {
                    self.death_loading = true;
                    self.death_presentation.exiting_world = true;
                    self.death_presentation.respawn_seconds = Some(0.0);
                    self.intents.disconnect = true;
                    self.dialog = None;
                }
            }
            MenuAction::ConfirmDeathQuit => {
                if self.dialog == Some(MenuDialog::DeathQuit) {
                    self.dialog = None;
                    self.intents.disconnect = true;
                    self.set_visible(false);
                }
            }
            MenuAction::DismissDialog | MenuAction::CloseExitDialog => self.dismiss_accounts(),
            MenuAction::Reconnect => {
                unreachable!("reconnect is handled before clearing the failure")
            }
            MenuAction::OpenAccounts => self.open_accounts(),
            MenuAction::AddAccount => self.add_account(),
            MenuAction::SwitchAccount(index) => self.switch_account(index),
            MenuAction::SelectServerTab(tab) => {
                self.server_tab = tab;
                self.focused = 0;
            }
            MenuAction::RefreshCatalog => {
                #[cfg(feature = "native-networking")]
                self.native_catalog.refresh();
                self.catalog_message = None;
            }
            MenuAction::StartSignIn => self.start_sign_in(),
            MenuAction::OpenSignInLink => self.update_sign_in_browser(true),
            MenuAction::CancelSignIn | MenuAction::CloseSignIn => {
                if self.feeds.account_adding {
                    self.cancel_add_account();
                } else {
                    self.stop_sign_in();
                }
                if self.dialog == Some(MenuDialog::Accounts) {
                    self.dialog = None;
                }
            }
            MenuAction::PlayAddServer => {
                self.editing = None;
                self.name.clear();
                self.address.clear();
                self.port.set_text(DEFAULT_PORT);
                self.enter(MenuScreen::AddServer);
                self.focus_field(MenuField::Name);
            }
            MenuAction::PlaySaved(index) => {
                if index < self.servers.len() {
                    self.servers[index].last_joined_unix = now_unix();
                    let address = self.servers[index].address.clone();
                    self.save_servers();
                    self.request_connect(address);
                }
            }
            MenuAction::PlayFeatured(index) => {
                if let Some(server) = self.featured.get(index) {
                    #[cfg(feature = "developer-control")]
                    bevy::log::info!(
                        featured_activation = %serde_json::json!({
                            "index": index,
                            "name": &server.name,
                            "logical_target": &server.address,
                        }),
                        "featured selection activated"
                    );
                    let address = server.address.clone();
                    #[cfg(feature = "native-networking")]
                    if self.request_native_gathering(&address) {
                        return;
                    }
                    self.request_featured_connect(address);
                }
            }
            MenuAction::RealmMembership(action) => self.activate_realm_membership(action),
            MenuAction::PlayRealm(index) => {
                if let Some(realm) = self.realms.get(index) {
                    let target = if realm.target.is_empty() {
                        realm.address.clone()
                    } else {
                        realm.target.clone()
                    };
                    if target.is_empty() {
                        self.message = Some(format!(
                            "{} is {} and cannot be joined right now.",
                            realm.name, realm.state
                        ));
                    } else {
                        self.request_connect(target);
                    }
                }
            }
            MenuAction::ToggleFavorite(index) => {
                if let Some(server) = self.servers.get_mut(index) {
                    server.favorite = !server.favorite;
                    self.message = Some(if server.favorite {
                        format!("{} added to Favorites.", server.name)
                    } else {
                        format!("{} removed from Favorites.", server.name)
                    });
                    self.save_servers();
                }
            }
            MenuAction::RemoveSavedDialog(index) => {
                if index < self.servers.len() {
                    self.dialog = Some(MenuDialog::RemoveSaved(index));
                    self.focused = 0;
                }
            }
            MenuAction::ConfirmRemoveSaved(index) => {
                if index < self.servers.len() {
                    let removed = self.servers.remove(index);
                    self.feeds.selected_saved = None;
                    self.save_servers();
                    self.message = Some(format!("Removed {}.", removed.name));
                }
                self.dialog = None;
            }
            MenuAction::PlayFriend(index) => {
                #[cfg(feature = "native-networking")]
                self.request_native_friend(index);
                #[cfg(not(feature = "native-networking"))]
                {
                    let _ = index;
                    self.message = Some("Native networking is unavailable in this build.".into());
                }
            }
            MenuAction::AddName
            | MenuAction::AddAddress
            | MenuAction::AddPort
            | MenuAction::EditSkinName
            | MenuAction::SearchFriends => {}
            MenuAction::AddSave => {
                // Saving pops the form back to the tab that opened it.
                if self.save_draft() {
                    self.go_back();
                }
            }
            MenuAction::AddSaveConnect => {
                if self.save_draft() {
                    self.request_connect(self.draft_endpoint());
                }
            }
            MenuAction::AddBack => self.go_back(),
            MenuAction::SelectRenderer(choice) => self.select_renderer(choice),
            MenuAction::ReloadShaders => self.activate_shader_reload(),
            MenuAction::ImportShaderPacks => self.request_shader_pack_import(),
            MenuAction::RemoveShaderPackDialog(index, revision) => {
                self.remove_shader_pack_dialog(index, revision)
            }
            MenuAction::ConfirmRemoveShaderPack(index, revision) => {
                self.confirm_remove_shader_pack(index, revision)
            }
            MenuAction::SettingsShaders => self.activate_shaders_tab(),
            MenuAction::ToggleRenderMode => {
                if render_model::ENHANCED_RENDERING_ENABLED {
                    self.render_mode = self.render_mode.toggled();
                    self.render_mode_request = Some(self.render_mode);
                }
            }
            // The game menu opened from the death screen returns to it.
            MenuAction::PauseResume if self.death_shown => {
                self.navigation_focus = navigation::NavigationFocus::default();
                self.history.reset(MenuScreen::Death);
                self.show_top();
            }
            MenuAction::PauseResume => self.set_visible(false),
            MenuAction::PauseDisconnect => {
                self.intents.disconnect = true;
                self.set_visible(false);
            }
            MenuAction::PauseSettings => {
                self.enter(MenuScreen::Settings);
            }
            MenuAction::EditSaved(index) => {
                if let Some(server) = self.servers.get(index) {
                    self.name.set_text(&server.name);
                    let (address, port) = split_address(&server.address);
                    self.address.set_text(&address);
                    self.port.set_text(&port);
                    self.enter(MenuScreen::AddServer);
                    self.editing = Some(index);
                    self.focus_field(MenuField::Name);
                }
            }
            MenuAction::SettingsStorage(action) => self.activate_storage(action),
            MenuAction::SettingsSupport(action) => self.activate_support(action),
            MenuAction::SettingsSection(section) => self.activate_settings_section(section),
            MenuAction::SettingsSearchSection(section) => {
                self.focus_pointer(MenuAction::SettingsSection(section));
                self.activate_settings_section(section);
            }
            action @ (MenuAction::SettingsScale(_)
            | MenuAction::SearchSettings
            | MenuAction::ClearSettingsSearch
            | MenuAction::SettingsScalePicker
            | MenuAction::SettingsFullscreen(_)
            | MenuAction::SettingsOption(..)
            | MenuAction::SettingsVideoPreset(_)
            | MenuAction::SettingsLanguage(_)
            | MenuAction::SettingsDropdown(_)
            | MenuAction::SettingsResetBindings(_)
            | MenuAction::SettingsResetGroup(_)
            | MenuAction::SettingsConfirmResetGroup(_)
            | MenuAction::SettingsConfirmResetBindings(_)
            | MenuAction::SettingsKey(_)
            | MenuAction::SettingsResetKey(_)
            | MenuAction::SettingsMacroCommand(_)
            | MenuAction::SettingsResetChat
            | MenuAction::SettingsAdvancedGraphics
            | MenuAction::EditTouchControls) => self.activate_settings(action),
            MenuAction::Respawn => {
                if self.death_controls_ready() {
                    self.request_respawn();
                }
            }
            MenuAction::SignOut => self.sign_out_requested = true,
            MenuAction::SelectFeatured(index) => self.feeds.select(index),
            MenuAction::SelectSaved(index) => self.feeds.select_saved(index),
            MenuAction::ServerList(action) => {
                self.settings_dirty |=
                    std::sync::Arc::make_mut(&mut self.settings_options).apply_server_list(action);
            }
            MenuAction::SelectRealm(index) => self.feeds.selected_realm = Some(index),
            MenuAction::ToggleReadMore(section) => self.feeds.toggle_read_more(section),
            MenuAction::OpenLiveEvent => self.open_live_event(),
            MenuAction::GlobalResources(action) => self.global_resource_actions.push(action),
            MenuAction::Mods(launcher::mods::Action::ViewUpdates) => {
                if !self.over_world() {
                    self.apply_global_resources_on_leave();
                    self.settings.section = launcher::menu::settings_sections::SettingSection::Mods
                        .definition()
                        .index;
                    self.enter(MenuScreen::Settings);
                }
            }
            MenuAction::Mods(launcher::mods::Action::OpenUpdate(id)) => {
                if let Some(update) = self
                    .mods
                    .library
                    .updates
                    .iter()
                    .find(|update| update.id == id)
                {
                    launcher_host::desktop::open_url(&update.release_url);
                }
            }
            MenuAction::Mods(action) => {
                if let Some(work) = std::sync::Arc::make_mut(&mut self.mods).apply(action) {
                    self.mod_actions.push(work);
                }
            }
            MenuAction::DressingRoom(action) => self.activate_dressing_room(action),
            MenuAction::PlayLocalWorld(index) => {
                if index < self.local_worlds.len() {
                    self.local_world_requested = Some(index);
                }
            }
            MenuAction::LocalWorld(action) => self.queue_local_action(action),
            MenuAction::WorldSettings(action) => self.activate_world_settings(action),
            MenuAction::ServerTrust(trusted) => self.answer_server_trust(trusted),
            MenuAction::Invite(action) => self.activate_invite(action),
            MenuAction::JoinRequest(accept) => self.answer_join_request(accept),
            MenuAction::ShaderOptions(action) => self.activate_shader_options(action),
        }
    }
}
