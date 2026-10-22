//! Registers the menu shell at the shared frame boundaries.
use crate::{fullscreen, menu, settings_runtime};
use bevy::prelude::{App, IntoScheduleConfigs, Last, Plugin, Startup, Update};
use client_state::{ClientFrameSet, ShellFrameSet};

/// Owns menu input, desktop settings and screen-focus publication.
pub struct ClientShellPlugin;
impl Plugin for ClientShellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<menu::MenuClipboard>()
            .init_resource::<client_state::ScreenFocus>()
            .init_resource::<client_state::GameplayScreenControls>()
            .add_plugins(crate::local_worlds::LocalWorldsPlugin)
            .add_systems(Startup, settings_runtime::initialize_render_distance)
            .add_systems(Last, menu::settings_options::flush_on_exit)
            .add_systems(
                Update,
                (
                    menu::observe_keyboard_labels,
                    fullscreen::toggle_fullscreen_hotkey,
                    menu::drive_menu_input,
                    menu::drive_menu_ime,
                    fullscreen::apply_runtime_fullscreen_setting,
                    settings_runtime::gui_scale::apply_gui_scale_setting,
                    menu::persist_video_settings,
                )
                    .chain()
                    .in_set(ClientFrameSet::UiAuthority)
                    .in_set(ShellFrameSet::MenuInput),
            )
            .add_systems(
                Update,
                settings_runtime::apply_window_settings
                    .in_set(ClientFrameSet::UiAuthority)
                    .in_set(ShellFrameSet::WindowSettings)
                    .after(ShellFrameSet::Session),
            )
            .add_systems(
                Update,
                menu::screen_focus::publish_screen_focus
                    .in_set(ClientFrameSet::UiAuthority)
                    .in_set(ShellFrameSet::Focus)
                    .after(ShellFrameSet::Session),
            )
            .add_systems(
                Update,
                menu::screen_focus::publish_screen_focus
                    .in_set(ClientFrameSet::SemanticFinalize)
                    .in_set(ShellFrameSet::FinalFocus),
            )
            .add_systems(
                Update,
                menu::screen_focus::publish_gameplay_screen_controls
                    .in_set(ShellFrameSet::GameplayControls),
            );
        crate::discord_presence::configure(app);
    }
}
