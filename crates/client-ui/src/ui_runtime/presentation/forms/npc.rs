//! Draws an active NPC dialogue form through the NPC screen and records its frame.

use json_ui::ViewState;
use protocol::NpcDialogueForm;
use ui::UiNode;

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::{ServerFormIdentity, UiRuntime};
use ui_engine::engine::screen::PaintFrame;
use ui_engine::text_metrics::TextMetrics;

impl UiPresentationRuntime {
    /// Draw `npc` through the engine; `Ok(false)` leaves it to the fallback.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_npc_dialogue(
        &mut self,
        runtime: &UiRuntime,
        npc: &NpcDialogueForm,
        identity: ServerFormIdentity,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<bool, UiPresentationError> {
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(false);
        };
        let view: &ViewState = &runtime.server_forms().engine().view;
        let translate = |key: &str| runtime.translation(key);
        let rollback = (nodes.len(), *next);
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes: &mut *nodes,
            next: &mut *next,
            renderers: &mut [],
        };
        match ui_screens::npc::paint(renderer, npc, view, &self.scene_clock, &mut paint_frame) {
            Ok(Some(frame)) => {
                self.projections.form.frame = Some((identity, frame));
                Ok(true)
            }
            Ok(None) | Err(_) => {
                nodes.truncate(rollback.0);
                *next = rollback.1;
                Ok(false)
            }
        }
    }
}
