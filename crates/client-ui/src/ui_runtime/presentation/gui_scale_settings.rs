//! Live GUI preference handoff. Rendering and pointer conversion both read
//! the presentation's preference and use the shared Bedrock desktop scale rule.

use super::UiPresentationRuntime;

#[cfg(test)]
mod tests;

impl UiPresentationRuntime {
    /// Native focus uses full control rectangles and the current screen's landmark tree.
    pub fn settings_focus_geometry(
        &self,
    ) -> (
        &[crate::menu::view::SettingsFocusTarget],
        &[crate::menu::view::SettingsFocusLandmark],
    ) {
        (
            &self.form_presentation.menu_focus_geometry,
            &self.form_presentation.menu_focus_landmarks,
        )
    }

    /// Whether the current topmost menu takes native Settings pointer clicks.
    pub fn uses_oreui_settings(&self) -> bool {
        self.form_presentation.oreui_settings_input
    }

    /// The unrounded drag position uses full thumb-centre endpoints before clipping.
    pub fn settings_slider_drag_fraction(&self, index: u16, point: ui::UiPoint) -> Option<f32> {
        let (_, track, _) = self
            .form_presentation
            .oreui_slider_tracks
            .iter()
            .find(|(at, _, _)| *at == index)?;
        let width = track.max().x() - track.min().x();
        (width > 0.0 && point.x().is_finite())
            .then(|| ((point.x() - track.min().x()) / width).clamp(0.0, 1.0))
    }

    /// Only the visible part of the current painted thumb starts a native drag.
    pub fn settings_slider_thumb_contains(&self, index: u16, point: ui::UiPoint) -> bool {
        self.form_presentation
            .oreui_slider_tracks
            .iter()
            .any(|(at, _, thumb)| *at == index && thumb.is_some_and(|thumb| thumb.contains(point)))
    }

    /// Visible animated thumbs take precedence over the stationary track while dragging.
    pub fn settings_slider_thumb_hit_test(&self, point: ui::UiPoint) -> Option<u16> {
        self.form_presentation
            .oreui_slider_tracks
            .iter()
            .rev()
            .find_map(|(index, _, thumb)| {
                thumb
                    .is_some_and(|thumb| thumb.contains(point))
                    .then_some(*index)
            })
    }

    /// Authored focus order includes controls keyboard navigation can scroll into view.
    pub fn menu_focus_actions(&self) -> impl Iterator<Item = crate::menu::MenuAction> + '_ {
        self.form_presentation.menu_focus.iter().copied().chain(
            self.menu_hit_targets
                .iter()
                .map(|(action, _)| *action)
                .filter(|_| self.form_presentation.menu_focus.is_empty()),
        )
    }

    /// Actions of the enabled controls in the most recently drawn menu.
    pub fn visible_menu_actions(&self) -> impl Iterator<Item = crate::menu::MenuAction> + '_ {
        self.menu_focus_actions()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn menu_action_bounds(&self, action: crate::menu::MenuAction) -> Option<ui::UiRect> {
        self.menu_hit_targets
            .iter()
            .rev()
            .find_map(|(candidate, bounds)| (*candidate == action).then_some(*bounds))
    }

    /// Returns the resolved GUI-scale track for app input integration tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn gui_scale_slider_track(&self) -> Option<ui::UiRect> {
        self.settings_slider_drag_targets
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
    /// Native scale option buttons publish no slider capture geometry.
    pub fn gui_scale_drag_action(&self, point: ui::UiPoint) -> Option<crate::menu::MenuAction> {
        captured_slider_action(
            self.settings_slider_drag_targets
                .iter()
                .filter(|(action, _)| matches!(action, crate::menu::MenuAction::SettingsScale(_))),
            point,
        )
    }

    /// A captured setting follows its full track horizontally until the pointer releases.
    pub fn settings_slider_drag_action(
        &self,
        index: u16,
        point: ui::UiPoint,
    ) -> Option<crate::menu::MenuAction> {
        let targets = if self.settings_slider_drag_targets.is_empty() {
            &self.menu_hit_targets
        } else {
            &self.settings_slider_drag_targets
        };
        captured_slider_action(
            targets.iter().filter(|(action, _)| {
                matches!(action, crate::menu::MenuAction::SettingsOption(at, _) if *at == index)
            }),
            point,
        )
    }
}

fn captured_slider_action<'a>(
    slider: impl DoubleEndedIterator<Item = &'a (crate::menu::MenuAction, ui::UiRect)> + Clone,
    point: ui::UiPoint,
) -> Option<crate::menu::MenuAction> {
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
