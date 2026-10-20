//! Original card artwork rendered through the production gameplay-card and panel painters.

use super::Fixture;
use json_ui::vanilla::Binding;
use serde_json::json;
use std::sync::Arc;
use ui::mod_panel::{
    CollectionDefinition, CollectionItem, Panel, Surface, SurfaceCollection, SurfaceValue,
};

/// Shows a small module grid beside a settings preview, with an empty comparison state.
pub(super) fn fixture(previews: bool) -> Fixture {
    let color = Binding::Color.as_str().trim_start_matches('#');
    let preview = |id: &str, size, offset| {
        json!({"type":if previews {"custom"} else {"panel"},
        "renderer":ui::mod_panel::hud_card::RENDERER,"card_id":id,"size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left"})
    };
    let label = |text: &str, offset, size: [i32; 2], scale| {
        json!({"type":"label","text":text,
        "size":size,"offset":offset,"anchor_from":"top_left","anchor_to":"top_left",
        "font_type":ui::mod_panel::SANS_FONT_NAME,"font_scale_factor":scale,(color):[0.9,0.93,1,1],"localize":false})
    };
    let document = json!({"namespace":"card_gallery","screen":{"type":"screen","size":[640,360],"controls":[
        {"background":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[620,340],"radius":12,(color):[0.06,0.07,0.09,1]}},
        {"title":label("Mods",[34,25],[180,28],1.6)},
        {"hint":label("Choose a module to adjust its appearance.",[34,58],[400,22],0.8)},
        {"grid":{"type":"panel","cinnabar_collection":"grid","size":[300,190],"offset":[34,118]}},
        {"stage":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[230,190],"offset":[170,24],"radius":10,(color):[0.11,0.13,0.17,1]}},
        {"stage_title":label("Keystrokes",[402,108],[200,24],1.)},
        {"large_preview":preview("keys",[210,120],[382,158])},
        {"stage_caption":label("Live preview",[402,292],[180,20],0.7)}
    ]}}).to_string();
    let tile = Surface {
        screen:"card_tile.root".into(), document:json!({"namespace":"card_tile","root":{"type":"panel","size":[140,90],"clips_children":true,"controls":[
            {"background":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[132,82],"radius":8,(color):[0.11,0.13,0.17,1]}},
            {"name":{"type":"label","text":"#item_name","bindings":[{"binding_name":"#item_name"}],"localize":false,"font_type":ui::mod_panel::SANS_FONT_NAME,"font_scale_factor":0.75,"size":[116,18],"offset":[0,-25],(color):[0.9,0.93,1,1]}},
            {"preview":{"type":if previews {"custom"} else {"panel"},"renderer":ui::mod_panel::hud_card::RENDERER,"size":[128,56],"offset":[0,8],"scale":0.75,
                "bindings":[{"binding_name":"#item_card","binding_name_override":"#card_id"}]}}
        ]}}).to_string(), bindings:Default::default(),previews:Default::default(),transitions:Default::default(),collections:Default::default(),
    };
    let items = [
        ("fps", "FPS"),
        ("cps", "CPS"),
        ("keys", "Keystrokes"),
        ("sprint", "Toggle sprint"),
    ]
    .map(|(id, name)| CollectionItem {
        values: [
            ("#item_card".into(), SurfaceValue::Text(id.into())),
            ("#item_name".into(), SurfaceValue::Text(name.into())),
        ]
        .into(),
        ..Default::default()
    });
    let surface = Surface {
        screen: "card_gallery.screen".into(),
        document,
        bindings: Default::default(),
        previews: Default::default(),
        transitions: Default::default(),
        collections: [(
            "grid".into(),
            SurfaceCollection {
                definition: Arc::new(CollectionDefinition {
                    template: tile,
                    columns: 2,
                    viewport: [300., 190.],
                    items: items.into(),
                }),
                order: (0..4).collect(),
            },
        )]
        .into(),
    };
    let hud = serde_json::from_value(json!({"cards":[
        {"id":"fps","rows":[{"label":"","value":"240 FPS"}],"text_plate":{"brackets":true,"background":[0.025,0.03,0.04,0.8]}},
        {"id":"cps","rows":[{"label":"","value":"8 | 6 CPS"}],"text_plate":{"brackets":true,"background":[0.025,0.03,0.04,0.8]}},
        {"id":"keys","width":66,"background_opacity":0,"rows":[],"cells":[
            {"rect":[23,0,20,20],"label":"W","background":[1,0.35,0.23,1]},
            {"rect":[0,23,20,20],"label":"A","background":[0.2,0.23,0.28,1]},
            {"rect":[23,23,20,20],"label":"S","background":[0.2,0.23,0.28,1]},
            {"rect":[46,23,20,20],"label":"D","background":[0.2,0.23,0.28,1]}]},
        {"id":"sprint","preview_only":true,"rows":[{"label":"","value":"Sprinting (Toggled)"}],"text_plate":{"background":[0.025,0.03,0.04,0.8]}}
    ]})).expect("valid original preview cards");
    let panel = Panel {
        surface: Some(surface),
        reference_size: Some([640., 360.]),
        title: "Mods".into(),
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
    Fixture::ModPanel(Box::new(panel), Some(Box::new(hud)), None)
}
