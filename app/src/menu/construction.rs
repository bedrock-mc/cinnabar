//! Launcher state construction and persisted settings startup.

use super::input::field_editor;
use super::*;

impl MenuRuntime {
    /// Creates a menu with the development install layout.
    #[cfg(test)]
    pub(crate) fn new(visible: bool, gui_scale: u8, display_name: String) -> Self {
        let player_skin = crate::player_skin::LocalPlayerSkin::generated_default(&display_name);
        Self::new_with_layout(
            visible,
            Some(gui_scale),
            display_name,
            InstallLayout::discover().expect("test executable must have a development layout"),
            player_skin,
        )
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
        let saved_video_settings =
            video_settings::load(&layout.user_config_root).unwrap_or_else(|error| {
                let warning = format!("Video settings could not be read: {error:#}");
                message = Some(message.take().map_or_else(
                    || warning.clone(),
                    |previous| format!("{previous}\n{warning}"),
                ));
                video_settings::SavedVideoSettings::default()
            });
        let settings_options = settings_options::SettingsOptions::load(
            &config_path.with_file_name(settings_options::SETTINGS_FILE),
        );
        let language_asset_path = layout.world_assets();
        let language_pending = settings_options.language().is_some();
        let language_choices =
            settings_options::SettingsOptions::language_choices(&layout.resource_root);
        Self {
            // The launcher owns the session lifecycle only when the client
            // started on the menu. `--address` keeps the historical behaviour
            // of exiting the process when its one session fails.
            launcher: visible,
            visible,
            screen: MenuScreen::Home,
            focused: 0,
            hovered: None,
            pressed: None,
            pointer_down: false,
            server_tab: MenuServerTab::Featured,
            profile_tab: ui::ProfileTab::default(),
            dialog: None,
            field: None,
            caret_revision: 0,
            history: {
                let mut history = json_ui::ScreenNav::default();
                history.reset(MenuScreen::Home);
                history
            },
            name: field_editor(MenuField::Name),
            address: field_editor(MenuField::Address),
            port: field_editor(MenuField::Port),
            message,
            gui_scale_preference: gui_scale
                .filter(|scale| *scale > 0)
                .map(|scale| scale.clamp(1, 4)),
            gui_scale_offset: saved_video_settings.gui_scale_offset,
            gui_scale_display_offset: saved_video_settings.gui_scale_offset,
            gui_scale_choices: vec![0],
            fullscreen: saved_video_settings.fullscreen,
            fullscreen_change: saved_video_settings.fullscreen.then_some(true),
            video_settings_writer: None,
            settings_focus: Vec::new(),
            last_saved_video_settings: saved_video_settings,
            failed_video_settings_save: None,
            render_mode: RenderMode::Vanilla,
            render_mode_request: None,
            display_name,
            servers: loaded.servers,
            saves: ServerWriter::new(config_path.clone(), loaded.allow_writes),
            config_path,
            pending_connect: None,
            connecting: false,
            disconnect_requested: false,
            exit_requested: false,
            session_generation: 1,
            transfer_hops_remaining: MAX_TRANSFER_CHAIN_HOPS,
            featured: Vec::new(),
            gatherings: Vec::new(),
            realms: Vec::new(),
            friends: Vec::new(),
            catalog_message: None,
            catalog_started: false,
            catalog_path: layout.catalog_file(std::process::id()),
            catalog_process: None,
            auth_process: None,
            auth_attempted: false,
            auth_restart_requested: false,
            layout,
            player_skin,
            session_directory: None,
            join: None,
            editing: None,
            settings_section: 0,
            disconnect_message: None,
            death_shown: false,
            respawn_requested: false,
            local_worlds: Vec::new(),
            local_world_requested: None,
            local_ui: Default::default(),
            control_auth: None,
            sign_in_page_code: None,
            sign_out_requested: false,
            store_actions: Vec::new(),
            global_resource_actions: Vec::new(),
            global_resources: Default::default(),
            store_snapshot: None,
            settings_options: std::sync::Arc::new(settings_options),
            storage: Default::default(),
            settings_dropdown: None,
            settings_dirty: false,
            settings_retry_at: None,
            settings_apply: true,
            language_choices,
            language_pending,
            language_asset_path,
            settings_slider_drag: None,
            key_remap: None,
            settings_advanced_graphics: false,
            local_world_joined: false,
            local_world_active: false,
            feeds: MenuFeeds::default(),
        }
    }
}
