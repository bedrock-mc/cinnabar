//! Retained rectangular HUD cells; geometry belongs to the publishing component.

use json_ui::{DataSource, Scalar};
use serde_json::{Value, json};
use ui::mod_hud::Card;

/// Builds centered text and square backgrounds with colors updated through bindings.
pub(super) fn controls(card: &Card, card_index: usize, header: f64) -> Vec<Value> {
    let k = f64::from(card.scale);
    let mut nodes = Vec::with_capacity(card.cells.len() * 3);
    for (index, cell) in card.cells.iter().enumerate() {
        let [x, y, width, height] = cell.rect.map(f64::from);
        let y = y + header;
        let prefix = format!("#cell_{card_index}_{index}");
        nodes.push(json!({format!("cell_{index}_background"):{
            "type":"custom","renderer":"cinnabar_rounded_rectangle","radius":0,
            "size":[width*k,height*k],"offset":[x*k,y*k],
            "anchor_from":"top_left","anchor_to":"top_left",
            "bindings":[{"binding_name":format!("{prefix}_background"),"binding_name_override":"#color"}]
        }}));
        for (second, text) in [(false, &cell.label), (true, &cell.value)] {
            if second && text.is_empty() {
                continue;
            }
            let key = format!("{prefix}_{}", if second { "value" } else { "label" });
            let center = y + height
                * if cell.value.is_empty() {
                    0.5
                } else if second {
                    0.78
                } else {
                    0.34
                };
            nodes.push(
                json!({format!("cell_{index}_{}",if second {"value"} else {"label"}):{
                    "type":"label","text":key,"size":[width*k,"default"],
                    "max_size":[width*k,height*k],"offset":[x*k,center*k],
                    "anchor_from":"top_left","anchor_to":"left_middle",
                    "text_alignment":"center","localize":false,"shadow":cell.shadow,
                    "font_scale_factor":0.8*k*f64::from(card.text_scale)*if second {0.5} else {1.},
                    "hide_hyphen":true,"clip_children":true,
                    "bindings":[{"binding_name":key},
                        {"binding_name":format!("{prefix}_color"),"binding_name_override":"#color"}]
                }}),
            );
        }
    }
    nodes
}

/// Publishes dynamic labels and colors without rebuilding the card catalog.
pub(super) fn data(data: &mut DataSource, card: &Card, card_index: usize) {
    for (index, cell) in card.cells.iter().enumerate() {
        let prefix = format!("#cell_{card_index}_{index}");
        data.set_global(format!("{prefix}_label"), Scalar::Text(cell.label.clone()));
        data.set_global(format!("{prefix}_value"), Scalar::Text(cell.value.clone()));
        data.set_global(format!("{prefix}_color"), Scalar::Json(json!(cell.color)));
        data.set_global(
            format!("{prefix}_background"),
            Scalar::Json(json!(cell.background)),
        );
    }
}
