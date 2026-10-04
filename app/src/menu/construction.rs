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
        let initial = MenuView::new(visible, display_name);
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
            name: field_editor(MenuField::Name),
            address: field_editor(MenuField::Address),
            port: field_editor(MenuField::Port),
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
            last_saved_video_settings: saved_video_settings,
            failed_video_settings_save: None,
            render_mode: initial.render_mode,
            render_mode_request: None,
            display_name: initial.display_name,
            servers: loaded.servers,
            saves: ServerWriter::new(config_path.clone(), loaded.allow_writes),
            config_path,
            intents: SessionIntents::default(),
            session: SessionStatus::default(),
            featured: initial.featured,
            gatherings: initial.gatherings,
            realms: initial.realms,
            friends: initial.friends,
            catalog_message: initial.catalog_message,
            catalog_started: false,
            catalog_path: layout.catalog_file(std::process::id()),
            catalog_process: None,
            auth_process: None,
            auth_attempted: false,
            auth_restart_requested: false,
            layout,
            player_skin,
            editing: initial.editing,
            settings_section: initial.settings_section,
            disconnect_message: initial.disconnect_message,
            death_shown: false,
            local_worlds: initial.local_worlds,
            local_world_requested: None,
            local_ui: Default::default(),
            control_auth: None,
            sign_in_page_code: None,
            sign_out_requested: false,
            store_actions: Vec::new(),
            global_resource_actions: Vec::new(),
            global_resources: initial.global_resources,
            store_snapshot: initial.store,
            settings_options: std::sync::Arc::new(settings_options),
            storage: initial.storage,
            settings_dropdown: initial.settings_dropdown,
            settings_dirty: false,
            settings_retry_at: None,
            settings_apply: true,
            language_choices,
            language_pending,
            language_asset_path,
            settings_slider_drag: None,
            key_remap: initial.key_remap,
            settings_advanced_graphics: initial.settings_advanced_graphics,
            local_world_joined: false,
            local_world_active: false,
            feeds: initial.feeds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_initial_view_matches_the_host_before_services_are_loaded() {
        let layout = crate::install_layout::scratch("initial-menu-view");
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
}
