//! Original artwork compares manual preview placement with measured tile and sheet fitting.

use super::Fixture;
use json_ui::vanilla::Binding;
use serde_json::json;

/// Shows the same published cards in small tiles and a larger capped preview stage.
pub(super) fn fixture(fitted: bool) -> Fixture {
    let color = Binding::Color.as_str().trim_start_matches('#');
    let mut controls = vec![json!({"background":{
        "type":"custom","renderer":json_ui::rounded::RENDERER,
        "size":["100%","100%"],(color):[0.055,0.065,0.085,1]
    }})];
    for (index, (id, title, size, offset, cap)) in [
        ("short", "Short text", [150, 72], [24, 60], 2.4),
        ("long", "Measured text plate", [150, 72], [202, 60], 2.4),
        (
            "rows",
            "Oversized multi-row card",
            [150, 100],
            [24, 206],
            2.4,
        ),
        (
            "padding",
            "Transparent padding",
            [150, 100],
            [202, 206],
            2.4,
        ),
        ("long", "Settings preview", [240, 180], [376, 100], 4.),
    ]
    .into_iter()
    .enumerate()
    {
        controls.push(json!({format!("label_{index}"):{
            "type":"label","text":title,"localize":false,
            "font_type":ui::mod_panel::SANS_FONT_NAME,"font_scale_factor":0.65,
            "size":[size[0],20],"offset":[offset[0],offset[1]-28],
            "anchor_from":"top_left","anchor_to":"top_left",(color):[0.85,0.9,1,1]
        }}));
        controls.push(json!({format!("tile_{index}"):{
            "type":"custom","renderer":json_ui::rounded::RENDERER,
            "size":size,"offset":offset,"radius":8,
            "anchor_from":"top_left","anchor_to":"top_left",(color):[0.12,0.14,0.18,1]
        }}));
        let mut preview = json!({"type":"custom","renderer":ui::mod_panel::hud_card::RENDERER,
            "card_id":id,"size":size,"offset":offset,"scale":1,
            "anchor_from":"top_left","anchor_to":"top_left"});
        if fitted {
            preview["fit"] = json!(0.78);
            preview["max_scale"] = json!(cap);
        }
        controls.push(json!({format!("preview_{index}"):preview}));
    }
    let panel = ui::mod_panel::Panel {
        surface: Some(ui::mod_panel::Surface {
            screen: "hud_fit.root".into(),
            document: json!({"namespace":"hud_fit","root":{
                "type":"screen","size":["100%","100%"],"controls":controls
            }})
            .to_string(),
            bindings: Default::default(),
            previews: Default::default(),
            transitions: Default::default(),
            collections: Default::default(),
        }),
        reference_size: Some([640., 360.]),
        title: "HUD previews".into(),
        toggle_key: "ShiftRight".into(),
        dark: true,
        controls: vec![],
        sections: vec![],
        capture_key: false,
        show_gameplay_hud: false,
        motion: false,
        theme: Default::default(),
        style: Default::default(),
    };
    let hud = serde_json::from_value(json!({"cards":[
        {"id":"short","rows":[{"label":"","value":"240 FPS"}],"text_plate":{"background":[0.02,0.03,0.04,0.9]}},
        {"id":"long","rows":[{"label":"","value":"Sprinting (Toggled)"}],"text_plate":{"brackets":true,"background":[0.02,0.03,0.04,0.9]}},
        {"id":"rows","width":220,"scale":5,"rows":(0..8).map(|index| json!({"label":format!("Row {index}"),"value":"123"})).collect::<Vec<_>>()},
        {"id":"padding","width":200,"background_opacity":0,"rows":[],"cells":[
            {"rect":[80,30,40,20],"label":"W","background":[1,0.35,0.23,1]}
        ]}
    ]}))
    .expect("valid original fitting cards");
    Fixture::ModPanel(Box::new(panel), Some(Box::new(hud)), None)
}
