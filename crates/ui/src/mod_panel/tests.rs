use super::*;

fn panel() -> Panel {
    Panel {
        surface: None,
        reference_size: None,
        theme: Default::default(),
        style: Default::default(),
        title: "Personal controls".into(),
        toggle_key: "ShiftRight".into(),
        dark: true,
        controls: vec![Control::Slider {
            id: "distance".into(),
            label: "Distance".into(),
            value: 3.0,
            min: 1.0,
            max: 6.0,
            step: 0.1,
        }],
        sections: Vec::new(),
        capture_key: false,
    }
}

#[test]
fn finite_slider_and_physical_key_are_accepted() {
    assert!(panel().validate().is_ok());
}

#[test]
fn invalid_slider_never_reaches_presentation() {
    for invalid in [f32::NAN, f32::INFINITY, -1.0, 8.0] {
        let mut panel = panel();
        if let Control::Slider { value, .. } = &mut panel.controls[0] {
            *value = invalid;
        }
        assert!(panel.validate().is_err());
    }
    for invalid in [0.0, -0.1, 10.0, f32::NAN, f32::INFINITY] {
        let mut panel = panel();
        if let Control::Slider { step, .. } = &mut panel.controls[0] {
            *step = invalid;
        }
        assert!(panel.validate().is_err());
    }
}

#[test]
fn ambiguous_identifiers_and_unbounded_text_are_rejected() {
    let mut panel = panel();
    panel.controls.push(panel.controls[0].clone());
    assert!(panel.validate().is_err());
    panel.controls.pop();
    panel.title.push('\n');
    assert!(panel.validate().is_err());
    panel.title = "x".repeat(MAX_PANEL_TEXT_BYTES + 1);
    assert!(panel.validate().is_err());
    panel.title = "Controls".into();
    panel.toggle_key = "Shift Right".into();
    assert!(panel.validate().is_err());
}

#[test]
fn choices_require_a_valid_selection_and_bounded_options() {
    let mut panel = panel();
    panel.controls = vec![Control::Choice {
        id: "mode".into(),
        label: "Mode".into(),
        index: 0,
        options: vec!["Continuous".into(), "While clicking".into()],
    }];
    assert!(panel.validate().is_ok());
    if let Control::Choice { index, .. } = &mut panel.controls[0] {
        *index = 2;
    }
    assert!(panel.validate().is_err());
    if let Control::Choice { index, options, .. } = &mut panel.controls[0] {
        *index = 0;
        options.clear();
    }
    assert!(panel.validate().is_err());
}

#[test]
fn sections_require_complete_unique_control_references() {
    let mut panel = panel();
    panel.sections = vec![Section {
        icon: Default::default(),
        id: "distance_section".into(),
        label: "Distance".into(),
        category: "Tools".into(),
        toggle: None,
        controls: vec!["distance".into()],
    }];
    assert!(panel.validate().is_ok());
    panel.sections[0].controls.push("distance".into());
    assert!(panel.validate().is_err());
    panel.sections[0].controls = vec!["missing".into()];
    assert!(panel.validate().is_err());
    panel.sections[0].controls.clear();
    assert!(panel.validate().is_err());
    panel.sections[0].toggle = Some("distance".into());
    assert!(panel.validate().is_err());
}

#[test]
fn compact_style_and_keybind_labels_are_bounded_and_opt_in() {
    let mut settings = panel();
    assert_eq!(settings.style, Style::Standard);
    settings.style = Style::Compact;
    settings.controls = vec![Control::Keybind {
        id: "binding".into(),
        label: "Keybind".into(),
        key: "KeyR".into(),
        capturing: true,
    }];
    assert!(settings.validate().is_ok());
    if let Control::Keybind { key, .. } = &mut settings.controls[0] {
        *key = "a".repeat(MAX_PANEL_ID_BYTES + 1);
    }
    assert!(settings.validate().is_err());
    if let Control::Keybind { key, .. } = &mut settings.controls[0] {
        *key = "R\n".into();
    }
    assert!(settings.validate().is_err());
}

#[test]
fn expanded_panels_still_reject_controls_above_the_capacity() {
    let mut panel = panel();
    panel.controls = (0..MAX_PANEL_CONTROLS)
        .map(|index| Control::Toggle {
            id: format!("toggle_{index}"),
            label: format!("Toggle {index}"),
            value: false,
        })
        .collect();
    assert!(panel.validate().is_ok());
    panel.controls.push(Control::Button {
        id: "extra".into(),
        label: "Extra".into(),
    });
    assert!(panel.validate().is_err());
}

/// Supplies a small extension-authored screen with one declared button route.
fn surface(action: &str) -> Surface {
    Surface {
        screen: "extension.screen".into(),
        document: serde_json::json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[{"action":{
                "type":"button","size":[32,16],"button_mappings":[{
                    "from_button_id":"button.menu_select","to_button_id":action,"mapping_type":"pressed"
                }]
            }}]
        }}).to_string(),
        bindings: Default::default(),
    }
}

#[test]
fn authored_surfaces_require_known_control_routes_and_keep_legacy_defaults() {
    let mut panel = panel();
    assert!(panel.surface.is_none());
    assert!(panel.reference_size.is_none());
    panel.surface = Some(surface("mod.control:0"));
    panel.validate().unwrap();
    panel.surface = Some(surface("mod.edit:0"));
    panel.validate().unwrap();
    for action in ["mod.control:1", "mod.edit:1", "hud.save", "unknown"] {
        panel.surface = Some(surface(action));
        assert!(panel.validate().is_err(), "unexpected route {action}");
    }
    panel.surface = Some(surface("mod.close"));
    panel.validate().unwrap();
}

#[test]
fn reference_size_requires_a_surface_and_finite_positive_bounded_dimensions() {
    let mut panel = panel();
    panel.reference_size = Some([495.0, 304.0]);
    assert!(panel.validate().is_err());
    panel.surface = Some(surface("mod.control:0"));
    panel.validate().unwrap();
    panel.reference_size = Some([1_000_000.0; 2]);
    panel.validate().unwrap();
    for invalid in [f32::NAN, f32::INFINITY, 0.0, -1.0, 1_000_001.0] {
        for axis in 0..2 {
            let mut size = [495.0, 304.0];
            size[axis] = invalid;
            panel.reference_size = Some(size);
            assert!(panel.validate().is_err(), "invalid axis {axis}: {invalid}");
        }
    }
}

#[test]
fn authored_surfaces_bound_values_expansion_and_structure() {
    let mut valid = surface("mod.control:0");
    for (name, value) in [
        ("#visible", SurfaceValue::Bool(true)),
        ("#count", SurfaceValue::Number(3.0)),
        ("#label", SurfaceValue::Text("A label".into())),
        ("#color", SurfaceValue::Vector(vec![1.0, 0.2, 0.3, 1.0])),
    ] {
        valid.bindings.insert(name.into(), value);
    }
    valid.validate().unwrap();
    for value in [
        SurfaceValue::Number(f64::NAN),
        SurfaceValue::Vector(vec![1.0; 5]),
        SurfaceValue::Text("x".repeat(257)),
    ] {
        let mut invalid = valid.clone();
        invalid.bindings.insert("#invalid".into(), value);
        assert!(invalid.validate().is_err());
    }
    let mut document: serde_json::Value = serde_json::from_str(&valid.document).unwrap();
    document["screen"]["factory"] = serde_json::json!({"name":"unbounded"});
    valid.document = document.to_string();
    assert!(valid.validate().is_err());
    document["screen"]
        .as_object_mut()
        .unwrap()
        .remove("factory");
    document["screen"]["controls"] =
        serde_json::json!([{"action@external.template":{"type":"button"}}]);
    valid.document = document.to_string();
    assert!(valid.validate().is_err());
    document["screen"]["controls"] =
        serde_json::json!([{"image":{"type":"custom","renderer":"external_renderer"}}]);
    valid.document = document.to_string();
    assert!(valid.validate().is_err());
}

#[test]
fn authored_surfaces_reject_recursive_depth_and_hidden_extra_definitions() {
    let mut authored = surface("mod.close");
    let mut nested = serde_json::json!({"type":"panel"});
    for _ in 0..34 {
        nested = serde_json::json!({"type":"panel","controls":[{"child":nested}]});
    }
    authored.document = serde_json::json!({"namespace":"extension","screen":nested}).to_string();
    assert!(authored.validate().is_err());
    authored.document = serde_json::json!({"namespace":"extension","screen":{"type":"screen"},"hidden":{"type":"panel"}}).to_string();
    assert!(authored.validate().is_err());
    authored.document = "{".into();
    assert!(authored.validate().is_err());
}

#[test]
fn authored_surfaces_reject_alias_amplification_and_inline_animation_bypasses() {
    let mut authored = surface("mod.close");
    let mut document = serde_json::json!({"namespace":"extension","screen":{
        "type":"panel", "$base":"x".repeat(4096), "$repeated":vec!["$base";32],
        "$amplified":vec!["$repeated";32], "unused":"$amplified"
    }});
    authored.document = document.to_string();
    assert!(authored.validate().is_err());
    document["screen"] = serde_json::json!({"type":"label","text":"$external"});
    authored.document = document.to_string();
    assert!(authored.validate().is_err());
    document["screen"] = serde_json::json!({"type":"panel", "property_bag":{"$nested":1}});
    authored.document = document.to_string();
    assert!(authored.validate().is_err());
    for animation in [
        serde_json::json!({"anim_type":"alpha","from":0,"to":1,"duration":1}),
        serde_json::json!("@extension.screen"),
    ] {
        document["screen"] = serde_json::json!({"type":"panel","alpha":animation});
        authored.document = document.to_string();
        assert!(authored.validate().is_err());
    }
}

#[test]
fn authored_surfaces_reject_feedback_bindings_and_keep_global_visibility_expressions() {
    let mut authored = surface("mod.close");
    let mut document = serde_json::json!({"namespace":"extension","screen":{
        "type":"label","property_bag":{"#growing":"x".repeat(4096)},
        "bindings":[{"binding_type":"view","source_property_name":"(#growing + #growing)","target_property_name":"#growing"}]
    }});
    authored.document = document.to_string();
    assert!(authored.validate().is_err());
    document["screen"]
        .as_object_mut()
        .unwrap()
        .remove("property_bag");
    for kind in ["view", "collection", "collection_details"] {
        document["screen"]["bindings"][0]["binding_type"] = serde_json::json!(kind);
        authored.document = document.to_string();
        assert!(authored.validate().is_err());
    }
    document["screen"]["bindings"] =
        serde_json::json!([{"binding_name":"(#menu_page = 0)","binding_name_override":"#visible"}]);
    authored.document = document.to_string();
    authored.validate().unwrap();
    document["screen"]["bindings"][0]["binding_type"] = serde_json::json!("global");
    authored.document = document.to_string();
    authored.validate().unwrap();
    document["screen"]["property_bag_for_children"] = serde_json::json!({"#payload":"large"});
    authored.document = document.to_string();
    assert!(authored.validate().is_err());
}

#[test]
fn scrollbar_track_actions_are_native_only_on_track_controls() {
    let mut panel = panel();
    let mut authored = surface("mod.scroll_track");
    let mut document: serde_json::Value = serde_json::from_str(&authored.document).unwrap();
    document["screen"]["controls"][0]["action"]["type"] = serde_json::json!("scroll_track");
    authored.document = document.to_string();
    panel.surface = Some(authored.clone());
    assert!(panel.validate().is_ok());
    document["screen"]["controls"][0]["action"]["type"] = serde_json::json!("button");
    authored.document = document.to_string();
    panel.surface = Some(authored);
    assert!(panel.validate().is_err());
}
