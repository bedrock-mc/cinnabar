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
fn label(style: ui::mod_hud::EditorStyle, text: &str, size: [f64; 2], offset: [f64; 2]) -> Value {
    json!({"type":"label","text":text,"size":[size[0],"default"],"max_size":size,
        "offset":[offset[0],offset[1]+size[1]*0.5-1.],"localize":false,
        "font_type":style.font.name(),"font_scale_factor":style.text_scale,"color":[1,1,1,1],"shadow":true,"text_alignment":"left",
        "anchor_from":"top_left","anchor_to":"left_middle","clip_children":true})
}
/// Gives native hit-testing a host-owned pressed action.
fn button(action: &str, size: [f64; 2], offset: [f64; 2], controls: Vec<Value>) -> Value {
    json!({"type":"button","size":size,"offset":offset,"controls":controls,
        "anchor_from":"top_left","anchor_to":"top_left",
        "button_mappings":[{"from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"}]})
}

/// Builds stable card previews and a bounded grid with editor controls.
pub(super) fn catalog(hud: &Hud, viewport: [f64; 2]) -> Result<Catalog, String> {
    let style = hud.editor_style.unwrap_or_default();
    let color = |value: [f32; 4]| value.map(f64::from);
    let mut controls = Vec::new();
    if hud.surface.is_none() {
        controls.push(named("shade", rect(viewport, [0.; 2], [0., 0., 0., 0.32])));
    }
    // A bounded grid. Larger virtual viewports keep the same drag increment and fewer drawn lines.
    if hud.surface.is_none() {
        let mut lines = Vec::new();
        for axis in 0..2 {
            let spacing =
                snapping::GRID * (viewport[axis] / (192. * snapping::GRID)).ceil().max(1.);
            for n in 1..=(viewport[axis] / spacing) as usize {
                let mut size = viewport;
                size[axis] = 0.5;
                let mut offset = [0.; 2];
                offset[axis] = n as f64 * spacing;
                lines.push(named(
                    &format!("line_{axis}_{n}"),
                    rect(size, offset, color(style.grid)),
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
        let mut node = rect(size, [0.; 2], color(style.guide));
        node["bindings"] = json!([
            {"binding_name":format!("#guide_{axis}_visible"),"binding_name_override":"#visible"},
            {"binding_name":format!("#guide_{axis}_offset"),"binding_name_override":"#offset"}
        ]);
        node["layer"] = json!(10);
        controls.push(named(&format!("guide_{axis}"), node));
    }
    controls.push(named(
        "keep_out",
        json!({
            "type":"custom", "renderer":"cinnabar_hud_keep_out", "size":viewport,
            "anchor_from":"top_left", "anchor_to":"top_left", "layer":1,
            "bindings":[{"binding_name":"#hud_dragging","binding_name_override":"#visible"}]
        }),
    ));
    for axis in 0..2 {
        let visible = json!({"binding_name":format!("#measurement_{axis}_visible"), "binding_name_override":"#visible"});
        let mut line = rect([1.; 2], [0.; 2], color(style.guide));
        line["layer"] = json!(10);
        line["bindings"] = json!([visible.clone(),
            {"binding_name":format!("#measurement_{axis}_offset"), "binding_name_override":"#offset"},
            {"binding_name":format!("#measurement_{axis}_width"), "binding_name_override":"#size_binding_x_absolute"},
            {"binding_name":format!("#measurement_{axis}_height"), "binding_name_override":"#size_binding_y_absolute"}]);
        controls.push(named(&format!("measurement_{axis}"), line));
        // The pill width follows its label; the label is centred in it at its natural height.
        let widest = feedback::pill_size("00000");
        let width = json!({"binding_name":format!("#measurement_{axis}_pill_width"),
            "binding_name_override":"#size_binding_x_absolute"});
        let mut text = label(
            style,
            &format!("#measurement_{axis}_label"),
            widest,
            [0.; 2],
        );
        text["offset"] = json!([0, 0]);
        text["size"][1] = json!("default");
        text["max_size"] = json!(widest);
        text["anchor_from"] = json!("center");
        text["anchor_to"] = json!("center");
        text["font_scale_factor"] = json!(MEASUREMENT_FONT_SCALE);
        text["clip_children"] = json!(false);
        text["text_alignment"] = json!("center");
        text["bindings"] = json!([{"binding_name":format!("#measurement_{axis}_label")}]);
        let mut background = rect(widest, [0.; 2], color(style.measurement));
        background["radius"] = json!(MEASUREMENT_PILL_HEIGHT * 0.5);
        background["bindings"] = json!([width.clone()]);
        controls.push(named(&format!("measurement_{axis}_pill"), json!({
            "type":"panel", "size":widest, "anchor_from":"top_left", "anchor_to":"top_left", "layer":11,
            "bindings":[visible, width, {"binding_name":format!("#measurement_{axis}_pill"),"binding_name_override":"#offset"}],
            "controls":[named("background",background),named("label",text)]
        })));
    }
    if style.selection_caption {
        let mut name = label(
            style,
            "#hud_caption_name",
            [30., caption::CAPTION_HEIGHT],
            [5., 0.],
        );
        name["max_size"] = json!(["100%", caption::CAPTION_HEIGHT]);
        name["bindings"] = json!([{"binding_name":"#hud_caption_name"},{"binding_name":"#hud_caption_name_width","binding_name_override":"#size_binding_x_absolute"}]);
        let mut scale = label(
            style,
            "#hud_selected_scale_text",
            [22., caption::CAPTION_HEIGHT],
            [0.; 2],
        );
        scale["text_alignment"] = json!("right");
        scale["size"][0] = json!("100%");
        scale["max_size"][0] = json!("100%");
        // Offset bindings replace the vector, so keep the measured vertical centering inside a wrapper.
        scale["bindings"] = json!([{"binding_name":"#hud_selected_scale_text"}]);
        for text in [&mut name, &mut scale] {
            text["color"] = json!(style.caption_text);
            text["shadow"] = json!(false);
        }
        let mut background = rect(
            [64., caption::CAPTION_HEIGHT],
            [0.; 2],
            color(style.selected),
        );
        background["size"][0] = json!("100%");
        background["radius"] = json!(3.5);
        controls.push(named("selection_caption",json!({"type":"panel","size":[64,caption::CAPTION_HEIGHT],"anchor_from":"top_left","anchor_to":"top_left","layer":30,
            "bindings":[{"binding_name":"#hud_caption_visible","binding_name_override":"#visible"},{"binding_name":"#hud_caption_offset","binding_name_override":"#offset"},{"binding_name":"#hud_caption_width","binding_name_override":"#size_binding_x_absolute"}],
            "controls":[named("background",background),named("name",name),named("scale",json!({"type":"panel","size":[22,caption::CAPTION_HEIGHT],"anchor_from":"top_left","anchor_to":"top_left","bindings":[{"binding_name":"#hud_caption_scale_offset","binding_name_override":"#offset"},{"binding_name":"#hud_caption_scale_width","binding_name_override":"#size_binding_x_absolute"}],"controls":[named("text",scale)]}))]
        })));
    }
    let mut row = 0;
    for (index, card) in hud.cards.iter().enumerate() {
        let visible = json!({"binding_name":format!("#hud_card_{index}_visible"), "binding_name_override":"#visible"});
        let mut artwork = cards::card(card, index, &mut row);
        artwork["bindings"]
            .as_array_mut()
            .expect("card bindings")
            .push(visible.clone());
        controls.push(named(&format!("card_{index}"), artwork));
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
        bounds["bindings"] = json!([{"binding_name":format!("#card_{index}_offset"),"binding_name_override":"#offset"}, visible.clone()]);
        controls.push(named(&format!("bounds_{index}"), bounds));
        if hud.resizable {
            for corner in 0..4 {
                let offset: [f64; 2] = std::array::from_fn(|axis| {
                    if corner & (1 << axis) == 0 {
                        0.
                    } else {
                        size[axis] - 8.
                    }
                });
                let mut visual = rect([5.; 2], [1.5; 2], color(style.handle));
                visual["radius"] = json!(style.handle_radius);
                let mut handle = button(
                    &format!("hud.resize:{index}:{corner}"),
                    [8.; 2],
                    offset,
                    vec![named("square", visual)],
                );
                handle["layer"] = json!(20);
                handle["default_control"] = json!("square");
                handle["hover_control"] = json!("hover");
                handle["pressed_control"] = json!("pressed");
                // The hit target keeps the mark's proportions, so round marks stay round.
                let feedback = |value| {
                    let mut node = rect([8.; 2], [0.; 2], color(value));
                    node["radius"] = json!(style.handle_radius * 8. / 5.);
                    node
                };
                handle["controls"].as_array_mut().unwrap().extend([
                    named("hover", feedback(style.handle_hover)),
                    named("pressed", feedback(style.handle_pressed)),
                ]);
                let mut group = json!({"type":"panel","size":size,"controls":[named("handle",handle)],"anchor_from":"top_left","anchor_to":"top_left","layer":20});
                group["bindings"] = json!([{"binding_name":format!("#card_{index}_offset"),"binding_name_override":"#offset"}, visible.clone()]);
                controls.push(named(&format!("resize_{index}_{corner}"), group));
            }
        }
        if !hud.hide_editor_labels && hud.surface.is_some() {
            let name = format!("#hud_card_{index}_name");
            let mut text = label(style, &name, [size[0], 12.], [0.; 2]);
            text["bindings"] = json!([{"binding_name":format!("#bounds_{index}_label_offset"),"binding_name_override":"#offset"}, {"binding_name":name}, visible]);
            controls.push(named(&format!("name_{index}"), text));
        }
    }
    if let Some(surface) = &hud.surface {
        controls.push(named(&format!("surface@{}", surface.screen), json!({})));
    } else {
        let actions: &[(&str, &str)] = if hud.autosave {
            &[
                ("close", "Done"),
                ("reset", "Reset"),
                ("grid", "#grid_label"),
                ("snap", "#snap_label"),
            ]
        } else {
            &[
                ("save", "Save"),
                ("cancel", "Cancel"),
                ("reset", "Reset"),
                ("grid", "#grid_label"),
                ("snap", "#snap_label"),
            ]
        };
        let slot = 53_f64.min((viewport[0] - 8.).max(0.) / actions.len() as f64);
        let width = actions.len() as f64 * slot + 8.;
        let x = ((viewport[0] - width) * 0.5).max(0.);
        let y = viewport[1] - 32.;
        let mut toolbar = vec![named(
            "surface",
            rect([width, 28.], [0.; 2], [0.055, 0.065, 0.08, 0.97]),
        )];
        for (index, &(action, text)) in actions.iter().enumerate() {
            let mut textnode = label(style, text, [(slot - 4.).max(0.), 12.], [4., 7.]);
            if matches!(action, "grid" | "snap") {
                textnode["bindings"] = json!([{"binding_name":text}]);
            }
            toolbar.push(named(
                action,
                button(
                    &format!("hud.{action}"),
                    [slot, 24.],
                    [4. + index as f64 * slot, 2.],
                    vec![named("text", textnode)],
                ),
            ));
        }
        controls.push(named(
            "toolbar",
            json!({"type":"panel","size":[width,28.],"offset":[x,y],
        "anchor_from":"top_left","anchor_to":"top_left","controls":toolbar}),
        ));
        if !hud.hide_editor_labels {
            let mut caption = label(
                style,
                "#editor_label",
                [390_f64.min(viewport[0] - 12.), 12.],
                [((viewport[0] - 390.) * 0.5).max(6.), viewport[1] - 61.],
            );
            caption["text_alignment"] = json!("center");
            caption["bindings"] = json!([{"binding_name":"#editor_label"}]);
            controls.push(named("selection_label", caption));
        }
        controls.push(named(
            "help",
            label(
                style,
                if hud.autosave {
                    "Drag / Shift bypasses snapping / arrows nudge / autosaved"
                } else {
                    "Drag / Shift bypasses snapping / Enter saves / Esc cancels"
                },
                [390_f64.min(viewport[0] - 12.), 12.],
                [((viewport[0] - 390.) * 0.5).max(6.), viewport[1] - 46.],
            ),
        ));
    }
    let definition=json!({"namespace":"cinnabar_hud_editor","layout":{"type":"panel","size":["100%","100%"],"controls":controls}}).to_string();
    let defs = if hud.surface.is_some() {
        br#"{"ui_defs":["ui/hud_editor.json","ui/extension_surface.json"]}"#.as_slice()
    } else {
        br#"{"ui_defs":["ui/hud_editor.json"]}"#.as_slice()
    };
    let surface_document = hud
        .surface
        .as_ref()
        .map(ui::mod_panel::Surface::resolved_document)
        .transpose()?;
    let mut files = vec![
        ("ui/_global_variables.json", &b"{}"[..]),
        ("ui/_ui_defs.json", defs),
        ("ui/hud_editor.json", definition.as_bytes()),
    ];
    if let Some(document) = &surface_document {
        files.push(("ui/extension_surface.json", document.as_bytes()));
    }
    let catalog = Catalog::from_files(files)
        .map_err(|e| e.to_string())?
        .with_paint_transforms();
    if !catalog.diagnostics().is_empty() {
        return Err(format!(
            "invalid HUD editor catalog: {}",
            catalog.diagnostics().join("; ")
        ));
    }
    Ok(catalog)
}
