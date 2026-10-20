//! Anonymous HUD editor states for retained history and visibility captures.

use super::Fixture;
use serde_json::json;

/// Builds a small original Studio surface that consumes the host's public bindings.
pub(super) fn fixture(edited: bool) -> Fixture {
    let label = |text: &str, offset: [u32; 2], binding: bool| {
        let mut node = json!({"type":"label","text":text,"size":[160,14],"offset":offset,
            "anchor_from":"top_left","anchor_to":"top_left","localize":false,"font_type":ui::mod_panel::SANS_FONT_NAME,"font_scale_factor":0.8,"color":[0.9,0.93,1]});
        if binding {
            node["bindings"] = json!([{"binding_name":text}]);
        }
        node
    };
    let mut controls = vec![json!({"title":label("HUD Layout", [24, 18], false)})];
    for (index, (text, action)) in [
        ("Undo", "hud.undo"),
        ("Redo", "hud.redo"),
        ("Center", "hud.anchor:4"),
        ("Scale +", "hud.scale_up"),
        ("Hide clock", "hud.visibility:1"),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("action_{index}");
        controls.push(json!({name:{"type":"button","size":[58,24],"offset":[160+index*62,12],
            "anchor_from":"top_left","anchor_to":"top_left",
            "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}],
            "controls":[{"text":label(text,[2,6],false)}]}}));
    }
    controls.extend([
        json!({"scale_title":label("Selected scale",[24,260],false)}),
        json!({"scale":label("#hud_selected_scale_text",[24,277],true)}),
        json!({"toast":label("#hud_toast",[240,300],true)}),
    ]);
    let hud: ui::mod_hud::Hud = serde_json::from_value(json!({
        "resizable":true, "hide_editor_labels":true,
        "surface":{"screen":"studio.root","document":json!({"namespace":"studio","root":{
            "type":"screen","size":["100%","100%"],"controls":controls
        }}).to_string(),"bindings":{}},
        "cards":[
            {"id":"fps","editor_label":"Frame rate","title":"Frame rate","width":130,"offset":[45,95],"rows":[{"label":"FPS","value":"144"}]},
            {"id":"clock","editor_label":"Clock","title":"Clock","width":130,"offset":[45,155],"rows":[{"label":"Time","value":"12:30"}]}
        ]
    })).unwrap();
    Fixture::HudEditor {
        hud: Box::new(hud),
        edited,
    }
}
