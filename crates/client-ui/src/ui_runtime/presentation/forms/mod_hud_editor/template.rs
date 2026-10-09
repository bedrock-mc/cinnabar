use super::*;
use serde_json::{Value, json};

/// Names a control in the private JSON-UI catalog.
fn named(name: &str, value: Value) -> Value {
    json!({name:value})
}
/// Defines bounded editor chrome through the native rectangle renderer.
fn rect(size: [f64; 2], offset: [f64; 2], color: [f64; 4]) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","radius":0,
        "size":size,"offset":offset,"color":color,"anchor_from":"top_left","anchor_to":"top_left"})
}
/// Creates a literal, clipped editor label using the active carrier font.
fn label(text: &str, size: [f64; 2], offset: [f64; 2]) -> Value {
    json!({"type":"label","text":text,"size":size,"offset":offset,"localize":false,
        "font_scale_factor":0.8,"color":[1,1,1,1],"shadow":true,"text_alignment":"left",
        "anchor_from":"top_left","anchor_to":"top_left","clip_children":true})
}
/// Gives native hit-testing a host-owned pressed action.
fn button(action: &str, size: [f64; 2], offset: [f64; 2], controls: Vec<Value>) -> Value {
    json!({"type":"button","size":size,"offset":offset,"controls":controls,
        "anchor_from":"top_left","anchor_to":"top_left",
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}]})
}

/// Builds stable card previews and a bounded grid with editor controls.
pub(super) fn catalog(editor: &HudEditor, viewport: [f64; 2]) -> Result<Catalog, String> {
    let hud = &editor.draft;
    let mut controls = Vec::new();
    if hud.surface.is_none() {
        controls.push(named("shade", rect(viewport, [0.; 2], [0., 0., 0., 0.32])));
    }
    // A bounded grid. Larger virtual viewports keep the same drag increment and fewer drawn lines.
    if hud.surface.is_none() {
        let mut lines = Vec::new();
        for axis in 0..2 {
            let spacing = 8. * (viewport[axis] / (192. * 8.)).ceil().max(1.);
            for n in 1..=(viewport[axis] / spacing) as usize {
                let mut size = viewport;
                size[axis] = 0.5;
                let mut offset = [0.; 2];
                offset[axis] = n as f64 * spacing;
                lines.push(named(
                    &format!("line_{axis}_{n}"),
                    rect(size, offset, [1., 1., 1., 0.09]),
                ));
            }
        }
        controls.push(named(
            "grid",
            json!({"type":"panel","size":viewport,"controls":lines,
        "bindings":[{"binding_name":"#grid_visible","binding_name_override":"#visible"}]}),
        ));
    }
    for axis in 0..2 {
        let mut size = viewport;
        size[axis] = GUIDE_WIDTH;
        let mut node = rect(size, [0.; 2], [0.2, 0.85, 1., 0.8]);
        node["bindings"] = json!([
            {"binding_name":format!("#guide_{axis}_visible"),"binding_name_override":"#visible"},
            {"binding_name":format!("#guide_{axis}_offset"),"binding_name_override":"#offset"}
        ]);
        node["layer"] = json!(10);
        controls.push(named(&format!("guide_{axis}"), node));
    }
    let mut row = 0;
    for (index, card) in hud.cards.iter().enumerate() {
        controls.push(named(
            &format!("card_{index}"),
            cards::card(card, index, &mut row),
        ));
        let size = cards::dimensions(card);
        let edges = [
            ([size[0], 0.5], [0., 0.]),
            ([size[0], 0.5], [0., size[1] - 0.5]),
            ([0.5, size[1]], [0., 0.]),
            ([0.5, size[1]], [size[0] - 0.5, 0.]),
        ];
        let border:Vec<_>=edges.into_iter().enumerate().map(|(n,(size,offset))| {
            let mut node=rect(size,offset,[1.;4]);
            node["bindings"]=json!([{"binding_name":format!("#bounds_{index}_color"),"binding_name_override":"#color"}]);
            named(&format!("edge_{n}"),node)
        }).collect();
        let mut bounds = button(&format!("hud.card:{index}"), size, [0.; 2], border);
        bounds["bindings"] = json!([{"binding_name":format!("#card_{index}_offset"),"binding_name_override":"#offset"}]);
        controls.push(named(&format!("bounds_{index}"), bounds));
        if !hud.hide_editor_labels {
            let text = if card.editor_label.is_empty() {
                &card.id
            } else {
                &card.editor_label
            };
            let mut text = label(text, [size[0], 12.], [0.; 2]);
            text["bindings"] = json!([{"binding_name":format!("#bounds_{index}_label_offset"),"binding_name_override":"#offset"}]);
            controls.push(named(&format!("name_{index}"), text));
        }
    }
    if let Some(surface) = &hud.surface {
        controls.push(named(&format!("surface@{}", surface.screen), json!({})));
    } else {
        let width = 220.;
        let x = ((viewport[0] - width) * 0.5).max(0.);
        let y = viewport[1] - 32.;
        let mut toolbar = vec![named(
            "surface",
            rect([width, 28.], [0.; 2], [0.055, 0.065, 0.08, 0.97]),
        )];
        for (index, (action, text)) in [
            ("save", "Save"),
            ("cancel", "Cancel"),
            ("reset", "Reset"),
            ("grid", "#grid_label"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut textnode = label(text, [49., 12.], [4., 7.]);
            if action == "grid" {
                textnode["bindings"] = json!([{"binding_name":"#grid_label"}]);
            }
            toolbar.push(named(
                action,
                button(
                    &format!("hud.{action}"),
                    [53., 24.],
                    [4. + index as f64 * 53., 2.],
                    vec![named("text", textnode)],
                ),
            ));
        }
        controls.push(named(
            "toolbar",
            json!({"type":"panel","size":[width,28.],"offset":[x,y],
        "anchor_from":"top_left","anchor_to":"top_left","controls":toolbar}),
        ));
        controls.push(named(
            "help",
            label(
                "Drag cards / arrows nudge / Enter saves / Esc cancels",
                [310_f64.min(viewport[0] - 12.), 12.],
                [((viewport[0] - 310.) * 0.5).max(6.), viewport[1] - 46.],
            ),
        ));
    }
    let definition=json!({"namespace":"cinnabar_hud_editor","layout":{"type":"panel","size":["100%","100%"],"controls":controls}}).to_string();
    let defs = if hud.surface.is_some() {
        br#"{"ui_defs":["ui/hud_editor.json","ui/extension_surface.json"]}"#.as_slice()
    } else {
        br#"{"ui_defs":["ui/hud_editor.json"]}"#.as_slice()
    };
    let mut files = vec![
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", defs),
        ("ui/hud_editor.json", definition.as_bytes()),
    ];
    if let Some(surface) = &hud.surface {
        files.push(("ui/extension_surface.json", surface.document.as_bytes()));
    }
    let catalog = Catalog::from_files(files).map_err(|e| e.to_string())?;
    if !catalog.diagnostics().is_empty() {
        return Err(format!(
            "invalid HUD editor catalog: {}",
            catalog.diagnostics().join("; ")
        ));
    }
    Ok(catalog)
}
