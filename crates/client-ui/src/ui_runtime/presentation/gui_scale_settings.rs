//! Live GUI preference handoff. Rendering and pointer conversion both read
//! the presentation's preference and use the shared Bedrock desktop scale rule.

use super::UiPresentationRuntime;

#[cfg(test)]
mod tests;

impl UiPresentationRuntime {
    /// Identifies the painted controls, so outgoing frames cannot drive another screen's focus.
    pub fn drawn_menu_context(&self) -> Option<(launcher::menu::MenuScreen, bool)> {
        self.projections.menu_focus_context
    }

    /// Native focus uses full control rectangles and the current screen's landmark tree.
    pub fn settings_focus_geometry(
        &self,
    ) -> (
        &[launcher::menu::view::SettingsFocusTarget],
        &[launcher::menu::view::SettingsFocusLandmark],
    ) {
        (
            &self.projections.menu_focus_geometry,
            &self.projections.menu_focus_landmarks,
        )
    }

    /// Whether the current topmost menu takes native Settings pointer clicks.
    pub fn uses_oreui_settings(&self) -> bool {
        self.projections.oreui_settings_input
    }

    /// The unrounded drag position uses full thumb-centre endpoints before clipping.
    pub fn settings_slider_drag_fraction(&self, index: u16, point: ui::UiPoint) -> Option<f32> {
        let (_, track, _) = self
            .projections
            .oreui_slider_tracks
            .iter()
            .find(|(action, _, _)| matches!(action, launcher::menu::MenuAction::SettingsOption(at, _) if *at == index))?;
        let width = track.max().x() - track.min().x();
        (width > 0.0 && point.x().is_finite())
            .then(|| ((point.x() - track.min().x()) / width).clamp(0.0, 1.0))
    }

    /// Only the visible part of the current painted thumb starts a native drag.
    pub fn settings_slider_thumb_contains(&self, index: u16, point: ui::UiPoint) -> bool {
        self.projections
            .oreui_slider_tracks
            .iter()
            .any(|(action, _, thumb)| {
                matches!(action, launcher::menu::MenuAction::SettingsOption(at, _) if *at == index)
                    && thumb.is_some_and(|thumb| thumb.contains(point))
            })
    }

    /// Visible animated thumbs take precedence over the stationary track while dragging.
    pub fn settings_slider_thumb_hit_test(&self, point: ui::UiPoint) -> Option<u16> {
        self.projections
            .oreui_slider_tracks
            .iter()
            .rev()
            .find_map(|(action, _, thumb)| {
                let launcher::menu::MenuAction::SettingsOption(index, _) = action else {
                    return None;
                };
                thumb
                    .is_some_and(|thumb| thumb.contains(point))
                    .then_some(*index)
            })
    }

    /// Authored focus order includes controls keyboard navigation can scroll into view.
    pub fn menu_focus_actions(&self) -> impl Iterator<Item = launcher::menu::MenuAction> + '_ {
        self.projections.menu_focus.iter().copied().chain(
            self.menu_hit_targets
                .iter()
                .map(|(action, _)| *action)
                .filter(|_| self.projections.menu_focus.is_empty()),
        )
    }

    /// Actions of the enabled controls in the most recently drawn menu.
    pub fn visible_menu_actions(&self) -> impl Iterator<Item = launcher::menu::MenuAction> + '_ {
        self.menu_focus_actions()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn menu_action_bounds(&self, action: launcher::menu::MenuAction) -> Option<ui::UiRect> {
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
                matches!(action, launcher::menu::MenuAction::SettingsScale(_)).then_some(*bounds)
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
    pub fn gui_scale_drag_action(&self, point: ui::UiPoint) -> Option<launcher::menu::MenuAction> {
        captured_slider_action(
            self.settings_slider_drag_targets
                .iter()
                .filter(|(action, _)| {
                    matches!(action, launcher::menu::MenuAction::SettingsScale(_))
                }),
            point,
        )
    }

    /// A captured setting follows its full track horizontally until the pointer releases.
    pub fn settings_slider_drag_action(
        &self,
        index: u16,
        point: ui::UiPoint,
    ) -> Option<launcher::menu::MenuAction> {
        let targets = if self.settings_slider_drag_targets.is_empty() {
            &self.menu_hit_targets
        } else {
            &self.settings_slider_drag_targets
        };
        captured_slider_action(
            targets.iter().filter(|(action, _)| {
                matches!(action, launcher::menu::MenuAction::SettingsOption(at, _) if *at == index)
            }),
            point,
        )
    }

    /// A captured shader option follows its full track, clamping to the pack's choice range.
    pub fn shader_option_slider_drag_action(
        &self,
        screen: u32,
        index: u16,
        point: ui::UiPoint,
    ) -> Option<launcher::menu::MenuAction> {
        use launcher::menu::MenuAction;
        use launcher::shader_options::{Action, Control, Input};
        let view = self.menu_view.as_ref()?.shader_options.as_ref()?;
        if view.screen != screen {
            return None;
        }
        let Control::Slider { count, .. } = view.elements.get(usize::from(index))?.control else {
            return None;
        };
        let (_, track, _) =
            self.projections
                .oreui_slider_tracks
                .iter()
                .find(|(action, _, _)| {
                    matches!(action, MenuAction::ShaderOptions(Action::Element {
                screen: at, index: element, input: Input::Select(_),
            }) if *at == screen && *element == index)
                })?;
        let width = track.width();
        if count <= 1 || width <= 0.0 {
            return None;
        }
        let fraction = ((point.x() - track.min().x()) / width).clamp(0.0, 1.0);
        Some(MenuAction::ShaderOptions(Action::Element {
            screen,
            index,
            input: Input::Select((fraction * f32::from(count - 1)).round() as u16),
        }))
    }
}

fn captured_slider_action<'a>(
    slider: impl DoubleEndedIterator<Item = &'a (launcher::menu::MenuAction, ui::UiRect)> + Clone,
    point: ui::UiPoint,
) -> Option<launcher::menu::MenuAction> {
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
