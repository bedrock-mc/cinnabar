use super::UiPresentationRuntime;
use crate::menu::MenuRuntime;
use bevy::{
    prelude::{Query, ResMut, With},
    window::{PrimaryWindow, Window},
};

pub(crate) fn apply_gui_scale_setting(
    mut menu: ResMut<MenuRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let physical = [window.physical_width(), window.physical_height()];
    let desktop = ui::DesktopGuiScale::for_window(physical);
    let scale = menu.gui_scale_preference().map_or_else(
        || desktop.scale_for_offset(menu.gui_scale_offset()),
        |fixed| ui::gui_scale(physical, Some(fixed)) as u8,
    );
    let displayed_offset = scale as i8 - desktop.scale_for_offset(0) as i8;
    menu.sync_gui_scale(displayed_offset, desktop.choices().collect());
    let preference = Some(scale);
    if presentation.gui_scale_preference() != preference {
        presentation.set_gui_scale_preference(preference);
    }
}
