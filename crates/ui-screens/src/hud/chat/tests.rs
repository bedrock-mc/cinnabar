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
    let mut chat = builtin;
    for control in filtered {
        chat.insert(control);
    }
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
