//! NPC dialogue through the vanilla NPC screen's student (player) view: the
//! NPC's name and dialogue text over one button per button-mode action.

use json_ui::{CollectionItem, DataSource, Scalar, ViewState};
use protocol::NpcDialogueForm;
use ui::UiNode;

use super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::engine;
use crate::ui_runtime::{ServerFormIdentity, UiRuntime};

pub const NPC_SCREEN: &str = "npc_interact.npc_screen";

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
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(false);
        };
        let data = npc_data(npc);
        let view: &ViewState = &runtime.server_forms().engine().view;
        let translate = |key: &str| runtime.translation(key);
        let rollback = (nodes.len(), *next);
        let inputs = engine::EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = engine::EngineOutput {
            nodes: &mut *nodes,
            next: &mut *next,
            overlay: &[],
        };
        let art = engine::ScreenArt {
            now: self.menu_seconds,
            clocks: Some(&self.scene_clock),
            ..engine::ScreenArt::default()
        };
        match renderer.render_screen(
            NPC_SCREEN,
            &data,
            &super::menu_screens::retail_context(),
            view,
            art,
            inputs,
            out,
        ) {
            Ok(Some(mut frame)) => {
                frame.identity = Some(identity);
                self.form_presentation.frame = Some(frame);
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

fn npc_data(npc: &NpcDialogueForm) -> DataSource {
    let mut data = DataSource::new();
    data.set_strict(true);
    data.set_global("#student_view_visible", Scalar::Bool(true));
    data.set_global("#title_text", Scalar::Text(npc.npc_name.to_string()));
    data.set_global("#dialogtext", Scalar::Text(npc.dialogue.to_string()));
    data.set_global("#action_count", Scalar::Num(npc.buttons.len() as f64));
    let buttons = npc
        .buttons
        .iter()
        .map(|button| {
            CollectionItem::default()
                .with(
                    "#student_button_text",
                    Scalar::Text(button.text.to_string()),
                )
                .with("#student_button_visible", Scalar::Bool(true))
        })
        .collect();
    data.set_collection("student_buttons_collection", buttons);
    data
}
