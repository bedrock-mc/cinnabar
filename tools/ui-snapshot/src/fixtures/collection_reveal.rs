//! A focused card just below the native collection viewport, before and after revealing it.

use serde_json::json;
use ui::mod_panel::{Panel, PanelPatch};

use super::Fixture;

/// Builds the same padded grid for both captures and optionally sends the guest reveal patch.
pub(super) fn fixture(reveal: bool) -> Fixture {
    let count = 33;
    let focused = 12;
    let cell = json!({"screen":"card.root","document":json!({"namespace":"card",
        "root":{"type":"panel","size":[111,97],"controls":[
            {"face":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
                "size":[103,89],"offset":[4,4],"anchor_from":"top_left","anchor_to":"top_left",
                "radius":10,"color":[0.12,0.14,0.16,1.0]}},
            {"focus":{"type":"custom","renderer":"cinnabar_rounded_rectangle",
                "size":[103,89],"offset":[4,4],"anchor_from":"top_left","anchor_to":"top_left",
                "radius":10,"color":[0.0,0.0,0.0,0.0],"border_width":2,"border_color":[1.0,0.35,0.24,1.0],
                "bindings":[{"binding_name":"#item_focused","binding_name_override":"#visible"}]}},
            {"label":{"type":"label","size":[90,20],"offset":[10,18],
                "anchor_from":"top_left","anchor_to":"top_left","text":"#item_name",
                "bindings":[{"binding_name":"#item_name"}]}}
        ]}}).to_string()});
    let panel: Panel = serde_json::from_value(json!({"title":"Collection focus","toggle_key":"F10",
        "reference_size":[640,360],"dark":true,"controls":[],
        "surface":{"screen":"menu.root","document":json!({"namespace":"menu",
            "root":{"type":"panel","size":[640,360],"controls":[
                {"heading":{"type":"label","text":"Collection focus","size":[452,24],"offset":[90,8],
                    "anchor_from":"top_left","anchor_to":"top_left"}},
                {"tiles":{"type":"panel","cinnabar_collection":"modules","offset":[90,40]}}
            ]}}).to_string(),"collections":{"modules":{"definition":{"template":cell,
                "columns":4,"viewport":[452,289],"items":(0..count).map(|i|json!({
                    "values":{"#item_name":format!("Card {}",i+1),"#item_focused":i==focused}
                })).collect::<Vec<_>>()},"order":(0..count).collect::<Vec<_>>()}}}})).unwrap();
    let panel = if reveal {
        PanelPatch::parse(
            &json!({"collection_reveal":{"collection":"modules","item":focused}}).to_string(),
        )
        .unwrap()
        .apply(&panel, &Default::default())
        .unwrap()
    } else {
        panel
    };
    Fixture::ModPanel(Box::new(panel), None, None)
}
