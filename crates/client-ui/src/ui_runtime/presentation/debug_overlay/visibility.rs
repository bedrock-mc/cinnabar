//! The developer overlay reads the same screen eligibility as UI rendering.

use super::super::UiPresentationRuntime;
use crate::ui_runtime::{UiRuntime, scene_stack::SceneHost};

impl UiPresentationRuntime {
    /// Tests current screen ownership without allocating or querying world statistics.
    pub fn debug_overlay_allowed(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
    ) -> bool {
        runtime.debug_overlay_allowed_in(player_runtime, self.scene_host(), &self.screen_settings())
    }

    /// Captures the presentation's current menu and loading authority for scene construction.
    pub(in super::super) fn scene_host(&self) -> SceneHost {
        SceneHost {
            menu: self.menu_view.as_ref().map(|view| view.screen),
            over_world: self.menu_view.as_ref().is_none_or(|view| view.over_world),
            loading: self.loading_stage.is_some(),
        }
    }
}
