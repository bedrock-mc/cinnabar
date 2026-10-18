use serde_json::json;

use super::*;

/// An original minimal built-in catalog with HUD chat and an open-chat screen.
fn builtin() -> Catalog {
    let mut catalog = Catalog::default();
    for (entry, document) in [
        (
            "ui/ui_common.json",
            json!({"namespace":"common","base_screen":{"type":"screen","size":["100%","100%"]}}),
        ),
        (
            "ui/chat_screen.json",
            json!({"namespace":"chat",
                "messages_text":{"type":"panel","controls":[{"text":{"type":"label","text":"#text"}}]},
                "chat_screen@common.base_screen":{"controls":[{"messages@chat.messages_text":{}}]}}),
        ),
        (
            "ui/hud_screen.json",
            json!({"namespace":"hud",
                "hud_screen":{"type":"screen","$additional_screen_content":"hud.java_chat",
                    "controls":[{"root_panel@hud.root_panel":{}}]},
                "root_panel":{"type":"panel","controls":[{"spacer":{"type":"panel"}}]},
                "java_chat":{"type":"panel","controls":[{"panel@hud.chat_panel":{}}]},
                "chat_panel":{"type":"stack_panel",
                    "factory":{"name":"chat_item_factory","control_ids":{"chat_item":"@hud.chat_grid_item"}}},
                "chat_grid_item":{"type":"panel","controls":[{"chat_text@hud.chat_label":{}}]},
                "chat_label":{"type":"label","text":"#text"}}),
        ),
    ] {
        catalog.overlay_text(entry, &document.to_string());
    }
    catalog
}

/// Layers one pack document over `builtin` the way the server pack stack does.
fn layered(builtin: &Catalog, entry: &str, document: serde_json::Value) -> Catalog {
    let mut catalog = builtin.clone();
    catalog.overlay_text(entry, &document.to_string());
    catalog
}

/// Assembles the protected screen catalog from the finisher's retained content.
fn builtin_chat(pack: &Catalog, builtin: &Catalog, retained: Vec<RawControl>) -> Catalog {
    let mut chat = pack.clone();
    for control in builtin.controls() {
        chat.insert(control.clone());
    }
    for control in retained {
        chat.insert(control);
    }
    chat
}

fn visible_when(expression: &str) -> serde_json::Value {
    json!({"binding_type":"view","source_property_name":expression,"target_property_name":"#visible"})
}

#[test]
fn unused_label_templates_do_not_filter_ordinary_chat() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "unused@hud.chat_label":{"bindings":[visible_when("(#text = 'unused')")]},
            "used@hud.chat_label":{"bindings":[visible_when("(not (#text = 'hidden'))")]},
            "custom_row":{"type":"panel","controls":[{"message@hud.used":{}}]},
            "root_panel":{"modifications":[{"array_name":"controls","operation":"insert_front",
                "value":[{"custom_chat":{"type":"stack_panel",
                    "factory":{"name":"chat_item_factory","control_ids":{"chat_item":"@hud.custom_row"}}}}]}]}}),
    );
    prefer_builtin(&mut pack, &builtin);
    let bindings = pack.lookup("hud", "chat_label").unwrap().props["bindings"]
        .as_array()
        .unwrap();
    assert!(bindings.contains(&visible_when("(not (#text = 'hidden'))")));
    assert!(!bindings.contains(&visible_when("(#text = 'unused')")));
}

#[test]
fn chat_screen_filters_survive_pack_overrides_of_shared_templates() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/ui_common.json",
        json!({"namespace":"common","base_screen":{"ignored":true}}),
    );
    pack.overlay_text(
        "ui/chat_screen.json",
        &json!({"namespace":"chat",
            "messages_text/text":{"bindings":[visible_when("(not (#text = 'hidden'))")]}})
        .to_string(),
    );
    let filtered = prefer_builtin(&mut pack, &builtin);
    assert!(!filtered.is_empty());
    let chat = builtin_chat(&pack, &builtin, filtered);
    let context = crate::menu_screens::retail_context();
    let screen = json_ui::resolve(&chat, "chat.chat_screen", &context)
        .control
        .expect("built-in chat keeps its own base screen");
    let text = screen
        .find(&|control: &ResolvedControl| control.name == "text")
        .expect("chat message label");
    assert!(
        text.properties
            .get("bindings")
            .and_then(Value::as_array)
            .is_some_and(|bindings| bindings.contains(&visible_when("(not (#text = 'hidden'))")))
    );
}

#[test]
fn copied_filters_keep_the_bindings_and_values_that_feed_them() {
    let pack_bindings = json!([
        {"binding_name":"#chat_text","binding_name_override":"#message"},
        {"binding_name":"#unrelated","binding_name_override":"#other"},
        {"binding_name":"#pack_text","binding_name_override":"#text"},
        visible_when("(not ((#message = #blocked) or (#text = 'y')))"),
    ]);
    let bag = json!({"#blocked":"x","#unused":1});
    let rules = Rules::read(Some(&pack_bindings), Some(&bag));
    assert_eq!(rules.bindings.len(), 3);
    assert!(!rules.bindings.contains(&pack_bindings[1]));
    assert_eq!(
        rules.bag,
        serde_json::Map::from_iter([("#blocked".to_owned(), json!("x"))])
    );
    let mut control = RawControl {
        owner_ns: "chat".into(),
        definition_scope: None,
        name: "text".into(),
        base: None,
        props: serde_json::Map::from_iter([(
            "bindings".to_owned(),
            json!([{"binding_name":"#chat_text","binding_name_override":"#text"}]),
        )]),
        children: Vec::new(),
        has_controls: false,
    };
    assert!(rules.merge_into(&mut control));
    let merged = control.props["bindings"].as_array().unwrap();
    assert!(merged.contains(&pack_bindings[0]));
    assert!(merged.contains(&pack_bindings[3]));
    assert!(
        !merged.contains(&pack_bindings[2]),
        "a dependency the control already binds keeps its own source"
    );
    assert_eq!(control.props["property_bag"], json!({"#blocked":"x"}));
}

#[test]
fn factory_variables_reach_the_filters_they_parameterize() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "used@hud.chat_label":{"bindings":[visible_when("(#text = $allowed)")]},
            "custom_row":{"type":"panel","controls":[{"message@hud.used":{}}]},
            "root_panel":{"modifications":[{"array_name":"controls","operation":"insert_front",
                "value":[{"custom_chat":{"type":"stack_panel","$allowed":"ordinary",
                    "factory":{"name":"chat_item_factory","control_ids":{"chat_item":"@hud.custom_row"},
                        "factory_variables":["$allowed"]}}}]}]}}),
    );
    prefer_builtin(&mut pack, &builtin);
    let bindings = pack.lookup("hud", "chat_label").unwrap().props["bindings"]
        .as_array()
        .unwrap();
    let source = bindings
        .iter()
        .find_map(|binding| binding["source_property_name"].as_str())
        .expect("copied filter");
    assert!(!source.contains("$allowed"), "unbound variable in {source}");
    assert!(
        source.contains("ordinary"),
        "lost captured value in {source}"
    );
}

#[test]
fn long_factory_chains_are_scanned_within_the_expansion_limit() {
    let builtin = builtin();
    let mut document = serde_json::Map::from_iter([("namespace".to_owned(), json!("hud"))]);
    const CHAIN: usize = 4_000;
    for index in 0..CHAIN {
        document.insert(
            format!("link_{index}"),
            json!({"type":"stack_panel",
                "factory":{"name":"chat_item_factory","control_ids":{"chat_item":format!("@hud.link_{}", index + 1)}}}),
        );
    }
    document.insert(
        format!("link_{CHAIN}@hud.chat_label"),
        json!({"bindings":[visible_when("(#text = 'deep')")]}),
    );
    document.insert(
        "root_panel".to_owned(),
        json!({"modifications":[{"array_name":"controls","operation":"insert_front",
            "value":[{"chain@hud.link_0":{}}]}]}),
    );
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        serde_json::Value::Object(document),
    );
    prefer_builtin(&mut pack, &builtin);
    let bindings = pack
        .lookup("hud", "chat_label")
        .unwrap()
        .props
        .get("bindings");
    assert!(
        !bindings
            .and_then(Value::as_array)
            .is_some_and(|bindings| bindings.contains(&visible_when("(#text = 'deep')")))
    );
}

#[test]
fn control_name_templates_see_their_enclosing_variables() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "used@hud.chat_label":{"bindings":[visible_when("(#text = $allowed)")]},
            "custom_row":{"type":"panel","controls":[{"message@hud.used":{}}]},
            "root_panel":{"modifications":[{"array_name":"controls","operation":"insert_front",
                "value":[{"custom_chat":{"type":"stack_panel","$allowed":"ordinary",
                    "factory":{"name":"chat_item_factory","control_name":"@hud.custom_row"}}}]}]}}),
    );
    prefer_builtin(&mut pack, &builtin);
    let bindings = pack.lookup("hud", "chat_label").unwrap().props["bindings"]
        .as_array()
        .unwrap();
    let source = bindings
        .iter()
        .find_map(|binding| binding["source_property_name"].as_str())
        .expect("copied filter");
    assert!(!source.contains("$allowed"), "unbound variable in {source}");
    assert!(
        source.contains("ordinary"),
        "lost enclosing value in {source}"
    );
}

#[test]
fn filter_dependencies_keep_full_property_names() {
    let bindings = json!([
        {"binding_name":"#chat_text","binding_name_override":"#custom:message"},
        visible_when("(#text = #custom:message)"),
    ]);
    let bag = json!({"#custom:message":"x","#custom":"y"});
    let rules = Rules::read(Some(&bindings), Some(&bag));
    assert_eq!(rules.bindings.len(), 2);
    assert_eq!(
        rules.bag,
        serde_json::Map::from_iter([("#custom:message".to_owned(), json!("x"))])
    );
}

#[test]
fn server_message_content_keeps_its_parser_and_hover_controls() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud","chat_grid_item":{
        "controls":[
            {"data":{"type":"panel","property_bag":{"#raw":"hidden|shown"},
                "bindings":[{"binding_type":"view","source_property_name":"(#raw - 'hidden|')",
                    "target_property_name":"#display"}]}},
            {"caption@hud.chat_label":{"bindings":[{"binding_type":"view",
                "source_control_name":"data","resolve_sibling_scope":true,
                "source_property_name":"#display","target_property_name":"#text"}]}}
        ]}}),
    );
    pack.overlay_text(
        "ui/chat_screen.json",
        &json!({"namespace":"chat",
        "messages_text":{"controls":[{"message@hover.row":{}}]}})
        .to_string(),
    );
    pack.overlay_text(
        "ui/hover_chat.json",
        &json!({"namespace":"hover",
        "row":{"type":"button","size":[60,10],"default_control":"idle","hover_control":"tooltip",
            "controls":[{"idle":{"type":"label","text":"shown"}},
                {"tooltip":{"type":"custom","renderer":"hover_text_renderer",
                    "property_bag":{"#hover_text":"detail"}}}]}})
        .to_string(),
    );
    let selected = prefer_builtin(&mut pack, &builtin);
    let row = json_ui::resolve(&pack, "hud.chat_grid_item", &json_ui::Context::empty())
        .control
        .unwrap();
    let bound = json_ui::bind(&row, &json_ui::DataSource::new(), &json_ui::EmptyLibrary);
    let caption = bound
        .find(&|control: &ResolvedControl| control.name == "caption")
        .expect("server caption survives built-in placement");
    assert_eq!(caption.properties["text"], "shown");
    let mut screen_catalog = pack;
    for control in builtin.controls() {
        screen_catalog.insert(control.clone());
    }
    for control in selected {
        screen_catalog.insert(control);
    }
    let screen = json_ui::resolve(
        &screen_catalog,
        "chat.chat_screen",
        &json_ui::Context::empty(),
    )
    .control
    .unwrap();
    assert!(
        screen
            .find(&|control: &ResolvedControl| control
                .properties
                .get("renderer")
                .is_some_and(|renderer| renderer == "hover_text_renderer"))
            .is_some()
    );
}

#[test]
fn encoded_message_fixture_binds_only_its_caption() {
    let files = crate::test_support::hover_chat::files();
    let catalog = Catalog::from_files(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    )
    .unwrap();
    let root = json_ui::resolve(&catalog, "hover.row", &json_ui::Context::empty())
        .control
        .unwrap();
    let mut data = json_ui::DataSource::new();
    data.set_control_values(
        "data",
        BTreeMap::from([(
            "#text".into(),
            json_ui::Scalar::Text(crate::test_support::hover_chat::message()),
        )]),
    );
    let bound = json_ui::bind(&root, &data, &json_ui::EmptyLibrary);
    assert_eq!(
        bound
            .find(&|control: &ResolvedControl| control.name == "caption")
            .unwrap()
            .properties["text"],
        crate::test_support::hover_chat::CAPTION
    );
}

#[test]
fn hud_factory_keeps_the_server_caption_across_refreshes() {
    let builtin = builtin();
    let mut pack = builtin.clone();
    let files = crate::test_support::hover_chat::files();
    pack.apply_pack(
        files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    prefer_builtin(&mut pack, &builtin);
    let context = json_ui::Context::empty();
    let root = json_ui::resolve(&pack, "hud.java_chat", &context)
        .control
        .unwrap();
    let mut data = json_ui::DataSource::new();
    data.set_collection(
        "chat_text_grid",
        vec![json_ui::CollectionItem::default().with(
            "#chat_text",
            json_ui::Scalar::Text(crate::test_support::hover_chat::message()),
        )],
    );
    data.set_factory(
        "chat_item_factory",
        vec![json_ui::FactoryItem::new("chat_item", 0.0).at("chat_text_grid", 0)],
    );
    let mut state = json_ui::BindState::new();
    for _ in 0..2 {
        let bound = json_ui::bind_screen(&root, &pack, &context, &data, &mut state);
        let caption = bound
            .find(&|control: &ResolvedControl| control.name == "caption")
            .unwrap();
        assert_eq!(
            caption.properties["text"],
            crate::test_support::hover_chat::CAPTION
        );
    }
}

#[test]
fn retained_rows_keep_named_caption_templates_without_duplicate_placements() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "caption@hud.chat_label":{"text":"Server caption"},
            "chat_grid_item":{"controls":[{"text@hud.caption":{}}]},
            "root_panel":{"controls":[{"duplicate@hud.chat_panel":{}}]}
        }),
    );
    prefer_builtin(&mut pack, &builtin);
    let context = json_ui::Context::empty();
    let row = json_ui::resolve(&pack, "hud.chat_grid_item", &context)
        .control
        .unwrap();
    let caption = row
        .find(&|control: &ResolvedControl| control.name == "text")
        .expect("referenced caption template remains available");
    assert_eq!(caption.properties["text"], "Server caption");
    let root = json_ui::resolve(&pack, "hud.root_panel", &context)
        .control
        .unwrap();
    assert!(
        root.find(&|control: &ResolvedControl| control.name == "duplicate")
            .is_none()
    );
}

#[test]
fn open_chat_keeps_inline_captions_inheriting_hud_labels() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/chat_screen.json",
        json!({"namespace":"chat",
            "messages_text":{"controls":[{"caption@hud.chat_label":{"text":"Server caption"}}]}
        }),
    );
    let retained = prefer_builtin(&mut pack, &builtin);
    let chat = builtin_chat(&pack, &builtin, retained);
    for catalog in [&pack, &chat] {
        let messages = json_ui::resolve(catalog, "chat.messages_text", &json_ui::Context::empty())
            .control
            .unwrap();
        let caption = messages
            .find(&|control: &ResolvedControl| control.name == "caption")
            .expect("inline caption survives placement suppression");
        assert_eq!(caption.properties["text"], "Server caption");
    }
}

#[test]
fn open_chat_grids_keep_caption_template_dependencies() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "caption@hud.chat_label":{"text":"Grid caption"}
        }),
    );
    pack.overlay_text(
        "ui/chat_screen.json",
        &json!({"namespace":"chat",
            "messages_text":{"controls":[{"grid":{"type":"grid", "grid_dimensions":[1,1],
                "grid_item_template":"hud.caption"}}]}
        })
        .to_string(),
    );
    let retained = prefer_builtin(&mut pack, &builtin);
    let chat = builtin_chat(&pack, &builtin, retained);
    for catalog in [&pack, &chat] {
        let context = json_ui::Context::empty();
        let messages = json_ui::resolve(catalog, "chat.messages_text", &context)
            .control
            .unwrap();
        let bound = json_ui::bind_screen(
            &messages,
            catalog,
            &context,
            &json_ui::DataSource::new(),
            &mut json_ui::BindState::new(),
        );
        assert!(
            bound
                .find(&|control: &ResolvedControl| control.properties.get("text")
                    == Some(&json!("Grid caption")))
                .is_some(),
            "grid caption survives placement suppression"
        );
    }
}

#[test]
fn scoped_message_dependencies_survive_suppression() {
    for contents in [
        json!({"$caption_template":"hud.caption","controls":[{"caption@$caption_template":{}}]}),
        json!({"$caption_template":"hud.caption","controls":[{"grid":{"type":"grid",
            "grid_dimensions":[1,1],"grid_item_template":"$caption_template"}}]}),
        json!({"$caption_template":"hud.caption","controls":[],"factory":{"name":"captions",
            "control_ids":{"caption":"$caption_template"}}}),
        json!({"$caption_template":"hud.caption","controls":[],"factory":{"name":"captions",
            "control_ids":{"caption":"chat.caption_host"},"factory_variables":["$caption_template"]}}),
        json!({"$contents":[{"caption@hud.caption":{}}],"controls":"$contents"}),
        json!({"$instance":"caption@hud.caption","controls":[{"$instance":{}}]}),
    ] {
        let builtin = builtin();
        let mut pack = layered(
            &builtin,
            "ui/hud_screen.json",
            json!({"namespace":"hud",
                "caption@hud.chat_label":{"text":"Scoped caption"}
            }),
        );
        pack.overlay_text(
            "ui/chat_screen.json",
            &json!({"namespace":"chat",
                "messages_text":contents,
                "caption_host":{"type":"panel","controls":[{"caption@$caption_template":{}}]}
            })
            .to_string(),
        );
        let retained = prefer_builtin(&mut pack, &builtin);
        let chat = builtin_chat(&pack, &builtin, retained);
        for catalog in [&pack, &chat] {
            let context = json_ui::Context::empty();
            let messages = json_ui::resolve(catalog, "chat.messages_text", &context)
                .control
                .unwrap();
            let mut data = json_ui::DataSource::new();
            data.set_factory("captions", vec![json_ui::FactoryItem::new("caption", 0.0)]);
            let bound = json_ui::bind_screen(
                &messages,
                catalog,
                &context,
                &data,
                &mut json_ui::BindState::new(),
            );
            assert!(
                bound
                    .find(&|control: &ResolvedControl| control.properties.get("text")
                        == Some(&json!("Scoped caption")))
                    .is_some(),
                "scoped caption survives: {contents}"
            );
        }
    }
}

#[test]
fn message_templates_cannot_replace_shared_chat_shell_definitions() {
    let builtin = builtin();
    let mut policy = ui_engine::engine::pack_catalog::CatalogPolicy::default();
    policy.after_layers(prefer_builtin);
    let files = vec![
        (
            json_ui::UI_DEFS_PATH.into(),
            br#"{"ui_defs":["ui/ui_common.json","ui/chat_screen.json"]}"#.to_vec(),
        ),
        (
            "ui/ui_common.json".into(),
            json!({"namespace":"common",
            "base_screen":{"ignored":true,"size":[43,17]}})
            .to_string()
            .into_bytes(),
        ),
        (
            "ui/chat_screen.json".into(),
            json!({"namespace":"chat",
            "messages_text":{"controls":[{"caption@common.base_screen":{
                "ignored":false,"text":"Isolated caption"}}]}})
            .to_string()
            .into_bytes(),
        ),
    ];
    let layered = policy.layer(&builtin, &[files]);
    let catalog = layered.builtin_screens.as_ref().unwrap();
    let root = json_ui::resolve(catalog, "chat.chat_screen", &json_ui::Context::empty())
        .control
        .expect("shared shell keeps its built-in base");
    assert_eq!(root.properties["size"], json!(["100%", "100%"]));
    let caption = root
        .find(&|control: &ResolvedControl| control.name == "caption")
        .unwrap();
    assert_eq!(caption.properties["text"], "Isolated caption");
    assert_eq!(caption.properties["size"], json!([43, 17]));
}

#[test]
fn cosmetic_rows_keep_active_ordinary_label_filters() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "unused@hud.chat_label":{"bindings":[visible_when("(#text = 'unused')")]},
            "used@hud.chat_label":{"bindings":[visible_when("(not (#text = 'hidden'))")]},
            "custom_row":{"type":"panel","controls":[{"message@hud.used":{}}]},
            "chat_grid_item/chat_text":{"color":[1,0,0]},
            "root_panel":{"controls":[{"custom_chat":{"type":"stack_panel",
                "factory":{"name":"chat_item_factory","control_ids":{"chat_item":"hud.custom_row"}}}}]}
        }),
    );
    prefer_builtin(&mut pack, &builtin);
    let context = json_ui::Context::empty();
    let root = json_ui::resolve(&pack, "hud.chat_grid_item", &context)
        .control
        .unwrap();
    let mut state = json_ui::BindState::new();
    for text in ["visible", "hidden", "visible"] {
        let mut data = json_ui::DataSource::new();
        data.set_creation_value("#text", json_ui::Scalar::Text(text.into()));
        let bound = json_ui::bind_screen(&root, &pack, &context, &data, &mut state);
        let label = bound
            .find(&|control: &ResolvedControl| control.name == "chat_text")
            .unwrap();
        assert_eq!(label.properties["color"], json!([1, 0, 0]));
        assert_eq!(
            bound
                .properties
                .get("visible")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            text != "hidden"
        );
    }
}

#[test]
fn retained_named_caption_filters_stay_independent() {
    let builtin = builtin();
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "one@hud.chat_label":{"bindings":[visible_when("(#text = 'one')")]},
            "two@hud.chat_label":{"bindings":[visible_when("(#text = 'two')")]},
            "chat_grid_item":{"controls":[{"first@hud.one":{}},{"second@hud.two":{}}]},
            "root_panel":{"controls":[{"chat":{"type":"stack_panel","factory":{
                "name":"chat_item_factory","control_ids":{"chat_item":"hud.chat_grid_item"}}}}]}
        }),
    );
    prefer_builtin(&mut pack, &builtin);
    let context = json_ui::Context::empty();
    let root = json_ui::resolve(&pack, "hud.chat_grid_item", &context)
        .control
        .unwrap();
    let mut state = json_ui::BindState::new();
    for text in ["one", "two", "one"] {
        let mut data = json_ui::DataSource::new();
        data.set_creation_value("#text", json_ui::Scalar::Text(text.into()));
        let bound = json_ui::bind_screen(&root, &pack, &context, &data, &mut state);
        assert_eq!(bound.children[0].properties["visible"], text == "one");
        assert_eq!(bound.children[1].properties["visible"], text == "two");
    }
}

#[test]
fn retained_unfiltered_captions_do_not_inherit_sibling_filters() {
    let builtin = builtin();
    let children = json!([
        {"chat_text@hud.chat_label":{}},
        {"conditional@hud.conditional":{}}
    ]);
    let mut pack = layered(
        &builtin,
        "ui/hud_screen.json",
        json!({"namespace":"hud",
            "conditional@hud.chat_label":{"bindings":[visible_when("(#text = 'shown')")]},
            "chat_grid_item":{"controls":children},
            "root_panel":{"controls":[{"chat":{"type":"stack_panel","factory":{
                "name":"chat_item_factory","control_ids":{"chat_item":"hud.chat_grid_item"}}}}]}
        }),
    );
    pack.overlay_text(
        "ui/chat_screen.json",
        &json!({"namespace":"chat","messages_text":{"controls":children}}).to_string(),
    );
    let retained = prefer_builtin(&mut pack, &builtin);
    let chat = builtin_chat(&pack, &builtin, retained);
    let context = json_ui::Context::empty();
    for (catalog, reference) in [(&pack, "hud.chat_grid_item"), (&chat, "chat.messages_text")] {
        let root = json_ui::resolve(catalog, reference, &context)
            .control
            .unwrap();
        let mut state = json_ui::BindState::new();
        for text in ["shown", "hidden", "shown"] {
            let mut data = json_ui::DataSource::new();
            data.set_creation_value("#text", json_ui::Scalar::Text(text.into()));
            let bound = json_ui::bind_screen(&root, catalog, &context, &data, &mut state);
            assert!(
                bound.children[0]
                    .properties
                    .get("visible")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                "unfiltered caption stays visible for {text} in {reference}"
            );
            assert_eq!(bound.children[1].properties["visible"], text == "shown");
        }
    }
}
