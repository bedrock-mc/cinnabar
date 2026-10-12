//! NPC dialogue through the vanilla NPC screen's student (player) view: the
//! NPC's name and dialogue text over one button per button-mode action.

use std::collections::BTreeMap;

use json_ui::{CollectionItem, DataSource, Scalar, ViewState};
use protocol::NpcDialogueForm;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    screen::{PaintError, PaintFrame},
};
use ui_engine::frame::EngineFrame;

pub const NPC_SCREEN: &str = "npc_interact.npc_screen";

/// Paints `npc` under the server form's `view`.
pub fn paint(
    engine: &FormEngine,
    npc: &NpcDialogueForm,
    view: &ViewState,
    clocks: &BTreeMap<String, f64>,
    frame: &mut PaintFrame<'_>,
) -> Result<Option<EngineFrame>, PaintError> {
    let data = npc_data(npc);
    let art = ScreenArt {
        clocks: Some(clocks),
        ..ScreenArt::default()
    };
    engine.render_screen(
        NPC_SCREEN,
        &data,
        &crate::menu_screens::retail_context(),
        view,
        art,
        frame,
    )
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
