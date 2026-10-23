//! Shows the roster while its key is held and nothing else owns the screen.

use launcher::menu::settings_options::SettingId;
use ui::UiNode;

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::player_list::PlayerList;

impl UiPresentationRuntime {
    /// Releases previous-session skin handles and their prepared atlas sources.
    pub(in super::super) fn clear_player_list(&mut self) {
        self.screens.player_list = None;
        if let Some(engine) = self.engine.as_deref() {
            engine.textures.lock().set_client_rasters(
                ui_screens::player_list::PORTRAIT_RASTER_NAMESPACE,
                &Default::default(),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn append_player_list(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        if !runtime.player_list_held()
            || runtime.ui_focused(player)
            || self.menu_view.is_some()
            || self.loading_stage.is_some()
            || self
                .screens
                .chat
                .settings
                .options()
                .value(SettingId::HideHud)
                != 0
        {
            return Ok(());
        }
        let top_inset = self.touch_utility_row_bottom(runtime, nodes, next, metrics, content)?;
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(());
        };
        let list = self
            .screens
            .player_list
            .get_or_insert_with(PlayerList::default);
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &|_| None,
            language: runtime.text_generation(),
            now: 0.0,
            nodes,
            next,
            renderers: &mut [],
        };
        list.paint(
            renderer,
            runtime.player_roster.rows(),
            &mut paint_frame,
            top_inset,
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "player_list_tests.rs"]
mod tests;
