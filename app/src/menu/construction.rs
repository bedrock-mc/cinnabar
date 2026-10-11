//! Launcher state construction and persisted settings startup.

use super::input::field_editor;
#[cfg(feature = "developer-control")]
use launcher::menu::MenuDialog;
use launcher::menu::settings_options::{SETTINGS_FILE, SettingsOptions};
use {
    super::*, launcher::install_layout::InstallLayout, launcher::menu::MenuField,
    launcher::menu::view::MenuView,
};

impl MenuRuntime {
    /// Creates a menu over the checkout's assets with private, empty user roots, so parallel test
    /// processes never share saved settings, servers or accounts.
    #[cfg(test)]
    pub(crate) fn new(visible: bool, gui_scale: u8, display_name: String) -> Self {
        let player_skin = crate::player_skin::LocalPlayerSkin::generated_default(&display_name);
        let mut layout = launcher::test_support::checkout();
        let user = launcher::test_support::scratch("menu");
        layout.user_config_root = user.user_config_root;
        layout.user_data_root = user.user_data_root;
        Self::new_with_layout(visible, Some(gui_scale), display_name, layout, player_skin)
    }

    /// Loads launcher state and both settings authorities for this install.
    pub(crate) fn new_with_layout(
        visible: bool,
        gui_scale: Option<u8>,
        display_name: String,
        layout: InstallLayout,
        player_skin: crate::player_skin::LocalPlayerSkin,
    ) -> Self {
        let config_path = layout.server_file();
        let loaded = load_servers(&config_path);
        let mut message = loaded.recovery_message;
        let saved_video_settings = launcher_host::video_settings::load(&layout.user_config_root)
            .unwrap_or_else(|error| {
                let warning = format!("Video settings could not be read: {error:#}");
                message = Some(message.take().map_or_else(
                    || warning.clone(),
                    |previous| format!("{previous}\n{warning}"),
                ));
                launcher_host::video_settings::SavedVideoSettings::default()
            });
        let settings_options = SettingsOptions::load(&config_path.with_file_name(SETTINGS_FILE));
        let language_asset_path = layout.world_assets();
        let language_pending = settings_options.language().is_some();
        let language_choices = SettingsOptions::language_choices(&layout.resource_root);
        let mut initial = MenuView::new(visible, display_name);
        #[cfg(feature = "developer-control")]
        let sign_in_fixture = super::sign_in_fixture::startup();
        #[cfg(feature = "developer-control")]
        let account_lookup = sign_in_fixture.is_none();
        #[cfg(not(feature = "developer-control"))]
        let account_lookup = true;
        if account_lookup && layout.auth_cache().is_file() {
            let store = launcher::accounts::AccountStore::new(layout.auth_cache());
            initial.feeds.accounts = store.list().unwrap_or_default();
            initial.feeds.account_active_id = store.active_id().ok().flatten();
        }
        #[cfg(feature = "developer-control")]
        if sign_in_fixture.is_some() {
            initial.dialog = Some(MenuDialog::Accounts);
            initial.feeds.account_adding = true;
        }
        Self {
            // The launcher owns the session lifecycle only when the client
            // started on the menu. `--address` keeps the historical behaviour
            // of exiting the process when its one session fails.
            launcher: visible,
            visible,
            screen: initial.screen,
            focused: 0,
            hovered: initial.hovered,
            pressed: initial.pressed,
            pointer_down: false,
            server_tab: initial.server_tab,
            profile_tab: initial.profile_tab,
            dialog: initial.dialog,
            field: initial.field,
            caret_revision: 0,
            history: {
                let mut history = json_ui::ScreenNav::default();
                history.reset(initial.screen);
                history
            },
            navigation_focus: navigation::NavigationFocus::default(),
            session_origin: None,
            retry_target: None,
            name: field_editor(MenuField::Name),
            address: field_editor(MenuField::Address),
            port: field_editor(MenuField::Port),
            skin_name: field_editor(MenuField::SkinName),
            message,
            gui_scale_preference: gui_scale
                .filter(|scale| *scale > 0)
                .map(|scale| scale.clamp(1, 4)),
            gui_scale_offset: saved_video_settings.gui_scale_offset,
            gui_scale_display_offset: saved_video_settings.gui_scale_offset,
            gui_scale_choices: initial.gui_scale_choices,
            fullscreen: saved_video_settings.fullscreen,
            fullscreen_change: saved_video_settings.fullscreen.then_some(true),
            video_settings_writer: None,
            settings_focus: Vec::new(),
            settings_focus_geometry: focus::SettingsFocusGeometry::default(),
            last_saved_video_settings: saved_video_settings,
            failed_video_settings_save: None,
            render_mode: initial.render_mode,
            render_mode_request: None,
            vsync_override: None,
            display_name: initial.display_name,
            servers: loaded.servers,
            saves: ServerWriter::new(config_path.clone(), loaded.allow_writes),
            config_path,
            intents: SessionIntents::default(),
            session: SessionStatus::default(),
            featured: initial.featured,
            realms: initial.realms,
            friends: initial.friends,
            catalog_message: initial.catalog_message,
            catalog_started: false,
            catalog: launcher_host::catalog::Catalog::new(layout.catalog_file(std::process::id())),
            catalog_art: None,
            auth_process: None,
            auth_attempted: false,
            auth_restart_requested: false,
            layout,
            player_skin,
            dressing_room: initial.dressing_room,
            dressing_room_worker: None,
            skin_update_pending: false,
            skin_outbound: None,
            skin_packet_pending: None,
            editing: initial.editing,
            settings_section: initial.settings_section,
            disconnect_message: initial.disconnect_message,
            death_shown: false,
            death_loading: false,
            death_presentation: launcher::menu::death::DeathPresentation::default(),
            death_retry_remaining: None,
            local_worlds: initial.local_worlds,
            local_world_requested: None,
            local_ui: Default::default(),
            control_auth: None,
            sign_in_browser: Default::default(),
            sign_in_requested: false,
            sign_in_cancelled: false,
            sign_in_failure: None,
            #[cfg(feature = "developer-control")]
            sign_in_fixture,
            presentation_accounts: false,
            sign_out_requested: false,
            accounts: Default::default(),
            store_actions: Vec::new(),
            global_resource_actions: Vec::new(),
            global_resources: initial.global_resources,
            store_snapshot: initial.store,
            settings_options: std::sync::Arc::new(settings_options),
            storage: initial.storage,
            settings_dropdown: initial.settings_dropdown,
            settings_scale_picker: initial.settings_scale_picker,
            settings_dirty: false,
            settings_retry_at: None,
            settings_apply: true,
            session_overrides: Vec::new(),
            transient_toggles: false,
            language_choices,
            language_pending,
            language_asset_path,
            settings_slider_drag: None,
            settings_slider_pointer: None,
            settings_slider_hovered: None,
            settings_slider_selected: None,
            settings_control_activation: None,
            settings_control_activation_navigation: false,
            settings_input_revision: 0,
            input_mode: input::MenuInputMode::default(),
            key_remap: initial.key_remap,
            settings_advanced_graphics: initial.settings_advanced_graphics,
            local_world_joined: false,
            local_world_active: false,
            feeds: initial.feeds,
            invite: Default::default(),
            realm_membership: Default::default(),
            join_requests: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, launcher::menu::view::MenuView};

    #[test]
    fn shared_initial_view_matches_the_host_before_services_are_loaded() {
        let layout = launcher::test_support::scratch("initial-menu-view");
        let display_name = "Initial view".to_owned();
        for visible in [false, true] {
            let initial = MenuView::new(visible, display_name.clone());
            let actual = MenuRuntime::new_with_layout(
                visible,
                Some(2),
                display_name.clone(),
                layout.clone(),
                crate::player_skin::LocalPlayerSkin::generated_default(&display_name),
            )
            .view();
            assert_eq!(actual.visible, initial.visible);
            assert_eq!(actual.over_world, initial.over_world);
            assert_eq!(actual.screen, initial.screen);
            assert_eq!(actual.profile_tab, initial.profile_tab);
            assert_eq!(actual.focused_action, initial.focused_action);
            assert_eq!(actual.caret, initial.caret);
            assert_eq!(actual.name, initial.name);
            assert_eq!(actual.address, initial.address);
            assert_eq!(actual.port, initial.port);
            assert_eq!(actual.local, initial.local);
            assert_eq!(actual.auth_state, initial.auth_state);
            assert_eq!(actual.catalog_loading, initial.catalog_loading);
        }
    }

    /// Parallel test processes must not see each other's saved settings.
    #[test]
    fn test_menus_never_share_saved_settings() {
        let mut first = MenuRuntime::new(true, 2, "Steve".to_owned());
        let default = first.settings_options.value("field_of_view");
        first.set_named_option("field_of_view", default + 1);
        first.sync_user_settings(None);
        assert!(!first.settings_dirty, "{:?}", first.message);

        let second = MenuRuntime::new(true, 2, "Steve".to_owned());
        assert_eq!(second.settings_options.value("field_of_view"), default);
    }
}
