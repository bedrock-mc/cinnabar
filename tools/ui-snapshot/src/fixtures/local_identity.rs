//! Original skin artwork and anonymous identity through the native panel painter.

use super::Fixture;
use json_ui::vanilla::Binding;
use serde_json::json;
use ui::mod_panel::{SANS_FONT_NAME, local_player_head as identity};

/// Shows the same personal panel with a neutral fallback or an admitted local portrait.
pub(super) fn fixture(available: bool) -> Fixture {
    let color = Binding::Color.as_str().trim_start_matches('#');
    let document = json!({"namespace":"local_identity","screen":{"type":"screen","controls":[
        {"card":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[360,156],
            "radius":12,(color):[0.055,0.07,0.1,1]}},
        {"content":{"type":"panel","size":[320,116],"controls":[
            {"title":{"type":"label","text":"Local player","size":[280,20],"offset":[0,0],
                "anchor_from":"top_left","anchor_to":"top_left","font_type":SANS_FONT_NAME,
                "font_scale_factor":0.8,(color):[0.6,0.67,0.8,1],"localize":false}},
            {"head":{"type":"image","texture":"","size":[64,64],"offset":[0,36],
                "anchor_from":"top_left","anchor_to":"top_left","bindings":[
                    {"binding_name":identity::HEAD,"binding_name_override":"#texture"},
                    {"binding_name":identity::AVAILABLE,"binding_name_override":"#visible"}]}},
            {"neutral":{"type":"custom","renderer":"cinnabar_rounded_rectangle","size":[64,64],
                "offset":[0,36],"radius":8,(color):[0.2,0.25,0.34,1],
                "anchor_from":"top_left","anchor_to":"top_left","bindings":[
                    {"binding_name":identity::UNAVAILABLE,"binding_name_override":"#visible"}]}},
            {"name":{"type":"label","text":identity::NAME,"size":[230,32],"offset":[84,52],
                "anchor_from":"top_left","anchor_to":"top_left","font_type":SANS_FONT_NAME,
                "font_scale_factor":1.3,(color):[0.94,0.96,1,1],"localize":false,
                "bindings":[{"binding_name":identity::NAME}]}}
        ]}}
    ]}})
    .to_string();
    let panel = serde_json::from_value(json!({
        "title":"Local player", "toggle_key":"F10", "dark":true, "controls":[],
        "reference_size":[640,360], "surface":{"screen":"local_identity.screen","document":document}
    }))
    .expect("valid identity panel fixture");
    Fixture::ModPanel(
        Box::new(panel),
        None,
        available.then(|| ("Local Player", portrait())),
    )
}

/// Extracts an original geometric face and hat using the production skin portrait code.
fn portrait() -> render_model::UiTexturePage {
    let side = render_api::CLASSIC_SKIN_SIDE;
    let mut rgba = vec![0; side * side * 4];
    for y in 8..16 {
        for x in 8..16 {
            let color = if y == 11 && (x == 10 || x == 13) {
                [15, 26, 47, 255]
            } else if y == 14 && (10..14).contains(&x) {
                [137, 67, 46, 255]
            } else {
                [222, 172, 109, 255]
            };
            let at = (y * side + x) * 4;
            rgba[at..at + 4].copy_from_slice(&color);
            if y <= 9 {
                let hat = (y * side + x + 32) * 4;
                rgba[hat..hat + 4].copy_from_slice(&[50, 111, 155, 255]);
            }
        }
    }
    render_model::actor::skin_portrait::extract(&render_api::StandardSkin {
        width: side as u32,
        height: side as u32,
        rgba8: rgba.into(),
        cape: None,
        geometry: None,
    })
    .expect("valid original portrait")
}
