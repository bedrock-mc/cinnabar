//! Live GUI preference handoff. Rendering and pointer conversion both read
//! the presentation's preference and use the shared Bedrock desktop scale rule.

use bevy::{
    prelude::{Query, ResMut, With},
    window::{PrimaryWindow, Window},
};

use super::UiPresentationRuntime;
use crate::menu::MenuRuntime;

impl UiPresentationRuntime {
    /// Actions of the enabled controls in the most recently drawn menu.
    pub(crate) fn visible_menu_actions(
        &self,
    ) -> impl Iterator<Item = crate::menu::MenuAction> + '_ {
        self.menu_hit_targets.iter().map(|(action, _)| *action)
    }

    #[cfg(test)]
    pub(crate) fn gui_scale_slider_track(&self) -> Option<ui::UiRect> {
        self.menu_hit_targets
            .iter()
            .filter_map(|(action, bounds)| {
                matches!(action, crate::menu::MenuAction::SettingsScale(_)).then_some(*bounds)
            })
            .reduce(|track, bounds| {
                super::rect(
                    track.min().x().min(bounds.min().x()),
                    track.min().y().min(bounds.min().y()),
                    track.max().x().max(bounds.max().x()),
                    track.max().y().max(bounds.max().y()),
                )
                .expect("slider hit targets already contain finite bounds")
            })
    }

    /// A captured slider follows the current layout after a scale change.
    /// Only its horizontal position matters while dragging; leaving either
    /// end of the track selects that end's value.
    pub(crate) fn gui_scale_drag_action(
        &self,
        point: ui::UiPoint,
    ) -> Option<crate::menu::MenuAction> {
        let targets = if self.gui_scale_drag_targets.is_empty() {
            &self.menu_hit_targets
        } else {
            &self.gui_scale_drag_targets
        };
        let slider = targets
            .iter()
            .filter(|(action, _)| matches!(action, crate::menu::MenuAction::SettingsScale(_)));
        let left = slider
            .clone()
            .map(|(_, bounds)| bounds.min().x())
            .reduce(f32::min)?;
        let right = slider
            .clone()
            .map(|(_, bounds)| bounds.max().x())
            .reduce(f32::max)?;
        let x = point.x().clamp(left, right);
        slider.rev().find_map(|(action, bounds)| {
            (x >= bounds.min().x() && x <= bounds.max().x()).then_some(*action)
        })
    }
}

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
    menu.sync_gui_scale(displayed_offset, desktop.offsets().collect());
    let preference = Some(scale);
    if presentation.gui_scale_preference != preference {
        presentation.set_gui_scale_preference(preference);
    }
}
