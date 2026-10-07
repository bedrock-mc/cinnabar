//! Live GUI preference handoff. Rendering and pointer conversion both read
//! the presentation's preference and use the shared Bedrock desktop scale rule.

use super::UiPresentationRuntime;

impl UiPresentationRuntime {
    /// Authored focus order, including controls keyboard navigation can scroll into view.
    pub fn menu_focus_actions(&self) -> impl Iterator<Item = crate::menu::MenuAction> + '_ {
        self.form_presentation
            .menu_focus_actions
            .iter()
            .copied()
            .chain(
                self.menu_hit_targets
                    .iter()
                    .filter(|_| self.form_presentation.menu_focus_actions.is_empty())
                    .map(|(action, _)| *action),
            )
    }

    /// Returns the resolved GUI-scale track for app input integration tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn gui_scale_slider_track(&self) -> Option<ui::UiRect> {
        self.slider_track(|action| matches!(action, crate::menu::MenuAction::SettingsScale(_)))
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Unites visible hit regions for the existing scale-slider witness.
    fn slider_track(
        &self,
        selected: impl Fn(crate::menu::MenuAction) -> bool,
    ) -> Option<ui::UiRect> {
        self.menu_hit_targets
            .iter()
            .filter_map(|(action, bounds)| selected(*action).then_some(*bounds))
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
    pub fn gui_scale_drag_action(&self, point: ui::UiPoint) -> Option<crate::menu::MenuAction> {
        self.slider_drag_action(point, |action| {
            matches!(action, crate::menu::MenuAction::SettingsScale(_))
        })
    }

    /// The captured settings slider keeps tracking outside its hover region.
    pub fn settings_slider_drag_action(
        &self,
        index: u16,
        point: ui::UiPoint,
    ) -> Option<crate::menu::MenuAction> {
        self.slider_drag_action(point, |action| matches!(action, crate::menu::MenuAction::SettingsOption(candidate, _) if candidate == index))
    }

    /// Resolves the horizontal value using the captured slider's full geometry.
    fn slider_drag_action(
        &self,
        point: ui::UiPoint,
        selected: impl Fn(crate::menu::MenuAction) -> bool,
    ) -> Option<crate::menu::MenuAction> {
        let targets = if self.settings_slider_drag_targets.is_empty() {
            &self.menu_hit_targets
        } else {
            &self.settings_slider_drag_targets
        };
        let slider = targets.iter().filter(|(action, _)| selected(*action));
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
