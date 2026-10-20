use super::*;
use std::collections::BTreeMap;

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
        show_gameplay_hud: false,
        motion: false,
    }
}

#[test]
fn finite_slider_and_physical_key_are_accepted() {
    assert!(panel().validate().is_ok());
}

#[test]
fn relative_axis_extent_bindings_require_a_declared_positive_numeric_value() {
    let mut panel = panel();
    panel.surface = Some(Surface { collections: Default::default(),
        previews: Default::default(),
screen:"extent.root".into(),
        document:serde_json::json!({"namespace":"extent","root":{
            "type":"button","size":[10,10],"button_mappings":[{
                "from_button_id":"button.menu_select","to_button_id":"mod.drag_axis:0:x:#resize_extent","mapping_type":"pressed"
            }]
        }}).to_string(),
        bindings:Default::default(),transitions:Default::default()
    });
    assert!(
        panel.validate().is_err(),
        "missing extent is not an implicit zero"
    );
    for value in [
        SurfaceValue::Bool(true),
        SurfaceValue::Text("112".into()),
        SurfaceValue::Vector(vec![112.0, 112.0]),
        SurfaceValue::Number(0.0),
        SurfaceValue::Number(-1.0),
        SurfaceValue::Number(f64::NAN),
        SurfaceValue::Number(1_000_001.0),
    ] {
        panel
            .surface
            .as_mut()
            .unwrap()
            .bindings
            .insert("#resize_extent".into(), value);
        assert!(panel.validate().is_err());
    }
    for value in [0.5, 112.0, 224.0, 1_000_000.0] {
        panel
            .surface
            .as_mut()
            .unwrap()
            .bindings
            .insert("#resize_extent".into(), SurfaceValue::Number(value));
        assert!(panel.validate().is_ok(), "extent={value}");
    }
}

/// Each route admits its own bounded controls before any sparse changes are applied.
#[test]
fn route_schemas_switch_atomically_and_keep_repeated_values() {
    let surface = serde_json::json!({"screen":"local.page","document":
        serde_json::json!({"namespace":"local","page":{"type":"button","button_mappings":[{"from_button_id":"button.menu_select","mapping_type":"pressed","to_button_id":"mod.control:0"}]}}).to_string()});
    let bundle = serde_json::json!({"panel":panel(),"surfaces":{"main":surface,"options":surface},
        "control_schemas":{"options":[{"kind":"toggle","id":"enabled","label":"Enabled","value":false}]}});
    let bundle = PanelBundle::parse(&bundle.to_string()).unwrap();
    let changed =
        PanelPatch::parse(r#"{"surface":"options","controls":[{"id":"enabled","value":true}]}"#)
            .unwrap()
            .apply_bundle(&bundle.panel, &bundle)
            .unwrap();
    assert!(bundle.matches_schema(&changed));
    assert!(matches!(
        changed.controls[0],
        Control::Toggle { value: true, .. }
    ));
    let repeated = PanelPatch::parse(r#"{"surface":"options"}"#)
        .unwrap()
        .apply_bundle(&changed, &bundle)
        .unwrap();
    assert_eq!(repeated.controls, changed.controls);
    assert!(
        PanelPatch::parse(r#"{"surface":"main","controls":[{"id":"enabled","value":false}]}"#)
            .unwrap()
            .apply_bundle(&changed, &bundle)
            .is_err()
    );
    let restored =
        PanelPatch::parse(r#"{"surface":"main","controls":[{"id":"distance","value":4}]}"#)
            .unwrap()
            .apply_bundle(&changed, &bundle)
            .unwrap();
    assert!(matches!(
        restored.controls[0],
        Control::Slider { value: 4., .. }
    ));
    let mut invalid = bundle.clone();
    invalid.control_schemas.insert("missing".into(), vec![]);
    assert!(invalid.validate().is_err());
    invalid = bundle.clone();
    invalid.control_schemas.get_mut("options").unwrap().clear();
    assert!(invalid.validate().is_err());
    invalid = bundle;
    invalid
        .control_schemas
        .get_mut("options")
        .unwrap()
        .resize(super::MAX_PANEL_CONTROLS + 1, changed.controls[0].clone());
    assert!(invalid.validate().is_err());
}

/// Selecting or repeating a route preserves capture until a patch explicitly changes it.
#[test]
fn bundled_routes_keep_sparse_capture_updates() {
    for initial_capture in [false, true] {
        let mut initial = panel();
        initial.capture_key = initial_capture;
        let bundle = PanelBundle {
            panel: initial,
            surfaces: BTreeMap::from([
                ("main".into(), surface("mod.control:0")),
                ("options".into(), surface("mod.close")),
            ]),
            control_schemas: BTreeMap::from([(
                "options".into(),
                vec![Control::Toggle {
                    id: "enabled".into(),
                    label: "Enabled".into(),
                    value: false,
                }],
            )]),
        };
        bundle.validate().unwrap();
        let patch = serde_json::json!({"capture_key": !initial_capture}).to_string();
        let mut current = PanelPatch::parse(&patch)
            .unwrap()
            .apply_bundle(&bundle.panel, &bundle)
            .unwrap();
        for route in ["main", "main", "options", "options", "main"] {
            let patch = serde_json::json!({"surface": route}).to_string();
            current = PanelPatch::parse(&patch)
                .unwrap()
                .apply_bundle(&current, &bundle)
                .unwrap();
            assert_eq!(current.capture_key, !initial_capture, "route {route}");
            assert_eq!(current.surface.as_ref(), Some(&bundle.surfaces[route]));
            assert!(bundle.matches_schema(&current));
            current.validate().unwrap();
        }
        for route in ["main", "options"] {
            let patch =
                serde_json::json!({"surface": route, "capture_key": initial_capture}).to_string();
            let changed = PanelPatch::parse(&patch)
                .unwrap()
                .apply_bundle(&current, &bundle)
                .unwrap();
            assert_eq!(changed.capture_key, initial_capture, "route {route}");
        }
    }
}

/// Compact scalar edits keep the exact kinds, ranges and transaction rules of named edits.
#[test]
fn indexed_control_values_are_typed_bounded_and_unambiguous() {
    let mut original = panel();
    original.controls.push(Control::Choice {
        id: "choice".into(),
        label: "Choice".into(),
        index: 0,
        options: vec!["A".into(), "B".into()],
    });
    let changed = PanelPatch::parse(r#"{"control_values":[[0,4],[1,1]]}"#)
        .unwrap()
        .apply(&original, &Default::default())
        .unwrap();
    assert!(matches!(
        changed.controls[0],
        Control::Slider { value: 4., .. }
    ));
    assert!(matches!(
        changed.controls[1],
        Control::Choice { index: 1, .. }
    ));
    for json in [
        r#"{"control_values":[[0,true]]}"#,
        r#"{"control_values":[[0,8]]}"#,
        r#"{"control_values":[[2,1]]}"#,
        r#"{"control_values":[[1,-1]]}"#,
        r#"{"control_values":[[1,0.5]]}"#,
        r#"{"control_values":[[1,2]]}"#,
        r#"{"control_values":[[0,4],[0,5]]}"#,
        r#"{"controls":[{"id":"distance","value":4}],"control_values":[[0,5]]}"#,
    ] {
        assert!(
            PanelPatch::parse(json)
                .unwrap()
                .apply(&original, &Default::default())
                .is_err(),
            "{json}"
        );
    }
    assert!(matches!(
        original.controls[0],
        Control::Slider { value: 3., .. }
    ));
}

/// Route and scalar changes preserve the schema, and invalid batches leave it untouched.
#[test]
fn sparse_panel_updates_validate_every_change_before_replacement() {
    let mut original = panel();
    original.controls.extend([
        Control::Toggle {
            id: "enabled".into(),
            label: "Enabled".into(),
            value: false,
        },
        Control::Keybind {
            id: "key".into(),
            label: "Key".into(),
            key: "KeyA".into(),
            capturing: false,
        },
        Control::Choice {
            id: "choice".into(),
            label: "Choice".into(),
            index: 0,
            options: vec!["A".into(), "B".into()],
        },
    ]);
    let patch = PanelPatch::parse(r#"{"controls":[{"id":"enabled","value":true},{"id":"key","key":"KeyC","capturing":true},{"id":"choice","index":1}],"capture_key":true}"#).unwrap();
    let changed = patch.apply(&original, &Default::default()).unwrap();
    assert_eq!(changed.controls[0], original.controls[0]);
    assert!(matches!(
        changed.controls[1],
        Control::Toggle { value: true, .. }
    ));
    assert!(
        matches!(&changed.controls[2], Control::Keybind { key, capturing: true, .. } if key == "KeyC")
    );
    assert!(matches!(
        changed.controls[3],
        Control::Choice { index: 1, .. }
    ));
    assert!(changed.capture_key);
    for invalid in [
        r#"{"controls":[{"id":"enabled","value":true},{"id":"missing","value":1}]}"#,
        r#"{"controls":[{"id":"enabled","value":1}]}"#,
        r#"{"controls":[{"id":"distance","value":8}]}"#,
        r#"{"controls":[{"id":"distance","key":"KeyB"}]}"#,
        r#"{"controls":[{"id":"choice","index":9}]}"#,
        r#"{"controls":[{"id":"enabled","value":true},{"id":"enabled","value":false}]}"#,
        r#"{"surface":"missing"}"#,
    ] {
        assert!(
            PanelPatch::parse(invalid)
                .unwrap()
                .apply(&original, &Default::default())
                .is_err()
        );
    }
    assert!(matches!(
        original.controls[1],
        Control::Toggle { value: false, .. }
    ));
}

/// Prepared routes admit only actions from their shared declared schema.
#[test]
fn local_panel_bundle_checks_routes_and_replaces_custom_bindings() {
    let document = |action: &str| {
        serde_json::json!({"namespace":"local","panel":{"type":"screen","controls":[{"button":{"type":"button","button_mappings":[{"from_button_id":"button.menu_select","mapping_type":"pressed","to_button_id":action}]}}]}}).to_string()
    };
    let bundle = |action: &str| {
        serde_json::json!({"panel":panel(),"surfaces":{"main":{"screen":"local.panel","document":document(action)}}}).to_string()
    };
    assert!(PanelBundle::parse(&bundle("mod.control:9")).is_err());
    let bundle = PanelBundle::parse(&bundle("mod.control:0")).unwrap();
    assert!(bundle.matches_schema(&bundle.panel));
    let mut relabeled = bundle.panel.clone();
    if let Control::Slider { label, .. } = &mut relabeled.controls[0] {
        *label = "Updated distance".into();
    }
    assert!(bundle.matches_schema(&relabeled));
    relabeled.controls.clear();
    assert!(!bundle.matches_schema(&relabeled));
    let patch = PanelPatch::parse(r##"{"surface":"main","bindings":{"#custom":true}}"##).unwrap();
    let changed = patch.apply(&bundle.panel, &bundle.surfaces).unwrap();
    assert_eq!(
        changed.surface.as_ref().unwrap().bindings["#custom"],
        SurfaceValue::Bool(true)
    );
    let unchanged = PanelPatch::parse(r#"{"surface":"main"}"#)
        .unwrap()
        .apply(&changed, &bundle.surfaces)
        .unwrap();
    assert_eq!(
        unchanged.surface.as_ref().unwrap().bindings["#custom"],
        SurfaceValue::Bool(true)
    );
    let updated = PanelPatch::parse(r##"{"binding_changes":{"#other":2}}"##)
        .unwrap()
        .apply(&unchanged, &bundle.surfaces)
        .unwrap();
    assert_eq!(
        updated.surface.as_ref().unwrap().bindings["#custom"],
        SurfaceValue::Bool(true)
    );
    assert_eq!(
        updated.surface.as_ref().unwrap().bindings["#other"],
        SurfaceValue::Number(2.)
    );
    assert!(
        !unchanged
            .surface
            .as_ref()
            .unwrap()
            .bindings
            .contains_key("#other")
    );
    assert!(
        PanelPatch::parse(r##"{"binding_changes":{"bad":true}}"##)
            .unwrap()
            .apply(&unchanged, &bundle.surfaces)
            .is_err()
    );
    assert!(
        PanelPatch::parse(r##"{"surface":"main","bindings":{"bad":true}}"##)
            .unwrap()
            .apply(&bundle.panel, &bundle.surfaces)
            .is_err()
    );
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
    Surface { collections: Default::default(), previews: Default::default(),
        transitions: Default::default(),
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
fn authored_surfaces_reject_recursive_depth_and_dynamic_unused_definitions() {
    let mut authored = surface("mod.close");
    let mut nested = serde_json::json!({"type":"panel"});
    for _ in 0..34 {
        nested = serde_json::json!({"type":"panel","controls":[{"child":nested}]});
    }
    authored.document = serde_json::json!({"namespace":"extension","screen":nested}).to_string();
    assert!(authored.validate().is_err());
    authored.document = serde_json::json!({"namespace":"extension","screen":{"type":"screen"},"hidden":{"type":"panel"}}).to_string();
    assert!(authored.validate().is_ok());
    authored.document = serde_json::json!({"namespace":"extension","screen":{"type":"screen"},"hidden":{"type":"panel","factory":{}}}).to_string();
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

#[test]
fn bounded_large_external_catalog_rejects_one_excess_control() {
    let mut panel = panel();
    panel.controls = (0..MAX_PANEL_CONTROLS)
        .map(|index| Control::Button {
            id: format!("action_{index}"),
            label: format!("Action {index}"),
        })
        .collect();
    assert!(panel.validate().is_ok());
    panel.controls.push(Control::Button {
        id: "excess".into(),
        label: "Excess".into(),
    });
    assert!(panel.validate().is_err());
}

#[test]
fn resolved_template_routes_still_require_declared_panel_controls() {
    let mut spec = panel();
    spec.surface=Some(Surface { collections: Default::default(), previews: Default::default(),transitions:Default::default(),screen:"extension.screen".into(),bindings:Default::default(),document:serde_json::json!({"namespace":"extension","button":{"type":"button","$action|default":"mod.control:0","button_mappings":[{"from_button_id":"button.menu_select","mapping_type":"pressed","to_button_id":"$action"}]},"screen":{"type":"panel","controls":[{"chosen@extension.button":{"$action":"mod.control:7"}}]}}).to_string()});
    assert!(spec.validate().is_err());
    let surface = spec.surface.as_mut().unwrap();
    surface.document = surface.document.replace("mod.control:7", "mod.control:0");
    assert!(spec.validate().is_ok());
}

/// A surface whose panel carries `transform` and a bound scale over a `child` control.
fn transformed(transform: serde_json::Value, child: &str) -> Surface {
    Surface {
        collections: Default::default(),
        previews: Default::default(),
        transitions: Default::default(),
        screen: "extension.screen".into(),
        document: serde_json::json!({"namespace":"extension","screen":{
            "type":"screen","size":["100%","100%"],"controls":[{"card":{
                "type":"panel","size":[40,20],"cinnabar_transform":transform,
                "bindings":[{"binding_name":"#press","binding_name_override":"#cinnabar_scale",
                    "binding_type":"global"}],
                "controls":[{"inner":{"type":child,"size":[10,10]}}]
            }}]
        }})
        .to_string(),
        bindings: Default::default(),
    }
}

#[test]
fn surfaces_keep_bounded_transforms_and_reject_rotated_clips() {
    let scaled = transformed(serde_json::json!({"scale": 0.97}), "scroll_view");
    scaled.validate().unwrap();
    assert!(
        scaled
            .resolved_document()
            .unwrap()
            .contains("cinnabar_transform"),
        "the canonical document keeps the transform"
    );
    transformed(serde_json::json!({"rotation": 45}), "label")
        .validate()
        .unwrap();
    for (transform, child) in [
        (serde_json::json!({"rotation": 45}), "scroll_view"),
        (serde_json::json!({"scale": 5}), "label"),
    ] {
        assert!(
            transformed(transform.clone(), child).validate().is_err(),
            "{transform} over {child}"
        );
    }
}

/// Collection viewports obey the same inherited clipping constraints as authored controls.
#[test]
fn surfaces_reject_rotated_collection_viewports() {
    use super::{CollectionDefinition, SurfaceCollection};

    let template = Surface {
        previews: Default::default(),
        screen: "extension.tile".into(),
        document: serde_json::json!({"namespace":"extension", "tile":{
            "type":"panel", "size":[20,20]
        }})
        .to_string(),
        bindings: Default::default(),
        transitions: Default::default(),
        collections: Default::default(),
    };
    let mut surface = transformed(serde_json::json!({"scale":0.97}), "panel");
    let mut document: serde_json::Value = serde_json::from_str(&surface.document).unwrap();
    document["screen"]["controls"][0]["card"]["controls"][0]["inner"]["cinnabar_collection"] =
        serde_json::json!("items");
    surface.collections.insert(
        "items".into(),
        SurfaceCollection {
            definition: std::sync::Arc::new(CollectionDefinition {
                template,
                columns: 1,
                viewport: [48.0, 40.0],
                items: vec![],
            }),
            order: vec![],
        },
    );
    surface.document = document.to_string();
    surface.validate().unwrap();
    surface.resolved_document().unwrap();
    for bound in [false, true] {
        let card = &mut document["screen"]["controls"][0]["card"];
        card["cinnabar_transform"] = serde_json::json!({"rotation":if bound {0} else {45}});
        if bound {
            card["bindings"][0]["binding_name_override"] = serde_json::json!("#cinnabar_rotation");
        }
        surface.document = document.to_string();
        assert!(
            surface.validate().is_err(),
            "rotated collection admitted (bound={bound})"
        );
        assert!(
            surface.actions().is_err(),
            "rotated collection actions admitted (bound={bound})"
        );
        assert!(
            surface.resolved_document().is_err(),
            "rotated collection document admitted (bound={bound})"
        );
    }
    let card = &mut document["screen"]["controls"][0]["card"];
    card["cinnabar_transform"] = serde_json::json!({"scale":0.97});
    card["bindings"][0]["binding_name_override"] = serde_json::json!("#cinnabar_scale");
    card["controls"][0]["inner"]["cinnabar_transform"] = serde_json::json!({"rotation":45});
    surface.document = document.to_string();
    assert!(surface.validate().is_err(), "rotated placeholder admitted");
    assert!(
        surface.resolved_document().is_err(),
        "rotated placeholder document admitted"
    );
}

/// Preview declarations belong to the immutable route definition, alongside collection data.
#[test]
fn surface_definition_tracks_previews_but_not_scalar_bindings() {
    let mut original = surface("mod.control:0");
    original.previews.insert(
        "sample".into(),
        preview::ControlPreview {
            reference: "fixture.first".into(),
            ..Default::default()
        },
    );
    let mut changed = original.clone();
    changed
        .bindings
        .insert("#value".into(), SurfaceValue::Number(2.));
    assert!(original.same_definition(&changed));
    changed.previews.get_mut("sample").unwrap().reference = "fixture.second".into();
    assert!(!original.same_definition(&changed));
}

/// Live and navigation capabilities are additive defaults and part of immutable route schemas.
#[test]
fn text_field_capabilities_default_off_and_preserve_schema() {
    let control: Control = serde_json::from_value(
        serde_json::json!({"kind":"text", "id":"search", "label":"Search", "value":""}),
    )
    .unwrap();
    assert!(matches!(
        &control,
        Control::Text {
            live: false,
            navigation_keys: false,
            ..
        }
    ));
    for flag in ["live", "navigation_keys"] {
        let mut changed = serde_json::to_value(&control).unwrap();
        changed[flag] = true.into();
        let changed: Control = serde_json::from_value(changed).unwrap();
        assert!(!control.same_schema(&changed));
    }
}

/// Motion is an optional typed value; failed transactions cannot change the retained panel.
#[test]
fn motion_patches_are_optional_typed_and_atomic() {
    for enabled in [false, true] {
        let mut original = panel();
        original.motion = enabled;
        for json in ["{}", r#"{"motion":null}"#] {
            let changed = PanelPatch::parse(json)
                .unwrap()
                .apply(&original, &Default::default())
                .unwrap();
            assert_eq!(changed, original);
        }
        for next in [false, true] {
            let patch = serde_json::json!({"motion":next,"control_values":[[0,4]]});
            let changed = PanelPatch::parse(&patch.to_string())
                .unwrap()
                .apply(&original, &Default::default())
                .unwrap();
            assert_eq!(changed.motion, next);
            assert!(matches!(
                changed.controls[0],
                Control::Slider { value: 4., .. }
            ));
            for invalid in [
                serde_json::json!({"motion":next,"control_values":[[0,8]]}),
                serde_json::json!({"motion":next,"surface":"missing"}),
                serde_json::json!({"motion":next,"control_values":[[0,4]],
                    "collection_order":{"missing":[0]}}),
            ] {
                let retained = original.clone();
                assert!(
                    PanelPatch::parse(&invalid.to_string())
                        .unwrap()
                        .apply(&original, &Default::default())
                        .is_err()
                );
                assert_eq!(original, retained);
            }
        }
    }
    for json in [
        r#"{"motion":0}"#,
        r#"{"motion":"false"}"#,
        r#"{"motion":[]}"#,
    ] {
        assert!(PanelPatch::parse(json).is_err());
    }
}

/// Route defaults and control schema switches cannot overwrite an existing motion preference.
#[test]
fn route_patches_retain_motion_until_explicitly_changed() {
    let surface = serde_json::json!({"screen":"local.page","document":
        serde_json::json!({"namespace":"local","page":{"type":"panel"}}).to_string()});
    for default in [false, true] {
        let mut initial = panel();
        initial.motion = default;
        let bundle = PanelBundle::parse(
            &serde_json::json!({
                "panel":initial,"surfaces":{"main":surface,"options":surface},
                "control_schemas":{"options":[{"kind":"button","id":"back","label":"Back"}]}
            })
            .to_string(),
        )
        .unwrap();
        let mut current = PanelPatch::parse(&serde_json::json!({"motion":!default}).to_string())
            .unwrap()
            .apply_bundle(&bundle.panel, &bundle)
            .unwrap();
        for route in ["main", "options", "options", "main"] {
            let patch = serde_json::json!({"surface":route});
            current = PanelPatch::parse(&patch.to_string())
                .unwrap()
                .apply_bundle(&current, &bundle)
                .unwrap();
            assert_eq!(current.motion, !default);
            let direct = PanelPatch::parse(&patch.to_string())
                .unwrap()
                .apply(&current, &bundle.surfaces)
                .unwrap();
            assert_eq!(direct.motion, !default);
        }
        let restored = PanelPatch::parse(
            &serde_json::json!({
                "surface":"options","motion":default
            })
            .to_string(),
        )
        .unwrap()
        .apply_bundle(&current, &bundle)
        .unwrap();
        assert_eq!(restored.motion, default);
        let retained = current.clone();
        assert!(
            PanelPatch::parse(
                &serde_json::json!({
                    "surface":"options","motion":default,"control_values":[[0,1]]
                })
                .to_string()
            )
            .unwrap()
            .apply_bundle(&current, &bundle)
            .is_err()
        );
        assert_eq!(current, retained);
    }
}
