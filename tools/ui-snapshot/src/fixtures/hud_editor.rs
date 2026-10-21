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
        ("Grid", "hud.grid"),
        ("Snap", "hud.snap"),
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
    if edited {
        let background = json!({"type":"custom","renderer":"cinnabar_rounded_rectangle",
            "radius":10,"size":[180,112],"color":[0.06,0.075,0.1,0.98]});
        controls.push(json!({"inspector_column":{
            "type":"stack_panel","orientation":"horizontal","size":["100%","100%"],
            "anchor_from":"top_left","anchor_to":"top_left","controls":[
                {"x":{"type":"panel","size":[0,1],"bindings":[{"binding_name":"#hud_inspector_x","binding_name_override":"#size_binding_x_absolute"}]}},
                {"column":{"type":"stack_panel","size":[180,"100%"],"controls":[
                    {"y":{"type":"panel","size":[180,0],"bindings":[{"binding_name":"#hud_inspector_y","binding_name_override":"#size_binding_y_absolute"}]}},
                    {"settings":{"type":"panel","size":[180,112],"controls":[
                        {"background":background},
                        {"heading":label("Frame rate settings",[12,12],false)},
                        {"scale":label("#hud_selected_scale_text",[12,38],true)},
                        {"hint":label("Anchor · Center",[12,64],false)},
                        {"hint2":label("Grid and snap are separate",[12,88],false)}
                    ]}}
                ]}}
            ]
        }}));
    }
    let hud: ui::mod_hud::Hud = serde_json::from_value(json!({
        "resizable":true, "hide_editor_labels":true,
        "inspector_size":if edited {Some([180,112])} else {None},
        "surface":{"screen":"studio.root","document":json!({"namespace":"studio","root":{
            "type":"screen","size":["100%","100%"],"controls":controls
        }}).to_string(),"bindings":{}},
        "cards":[
            {"id":"fps","editor_label":"Frame rate","title":"Frame rate","width":130,"offset":[45,95],"rows":[{"label":"FPS","value":"144"}]},
            {"id":"clock_card","editor_label":"Clock","title":"Clock","width":130,"offset":[45,155],"rows":[{"label":"Time","value":"12:30"}]}
        ]
    })).unwrap();
    Fixture::HudEditor {
        hud: Box::new(hud),
        edited,
    }
}
