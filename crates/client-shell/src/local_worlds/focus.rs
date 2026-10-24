//! Desktop focus and pause preferences for the local world's existing controller.

use super::LocalWorlds;
use bevy::{
    prelude::{MessageReader, Res, ResMut},
    window::WindowFocused,
};
use launcher::menu::settings_options::SettingId;

/// Applies the desktop focus preference before forwarding focus changes to the local world.
pub(super) fn pause_on_focus(
    mut focus: MessageReader<WindowFocused>,
    mut worlds: ResMut<LocalWorlds>,
    menu: Option<Res<crate::menu::MenuRuntime>>,
    desktop_focus: Option<Res<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
) {
    if let Some(menu) = menu {
        worlds.pause_on_unfocus = menu
            .settings_snapshot()
            .0
            .value(SettingId::PauseMenuOnFocusLost)
            != 0;
        worlds.sync_pause();
    }
    if let Some(desktop_focus) = desktop_focus {
        focus.clear();
        worlds.focus_changed(driven.is_some() || desktop_focus.available());
    } else {
        for message in focus.read() {
            worlds.focus_changed(message.focused);
        }
    }
}
