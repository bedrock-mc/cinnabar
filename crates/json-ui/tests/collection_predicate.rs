use json_ui::{CollectionItem, DataSource, EmptyLibrary, ResolvedControl, Scalar, bind};
use serde_json::{Value, json};

fn collection_button(expression: &str) -> ResolvedControl {
    let properties = json!({
        "visible": false,
        "bindings": [{
            "binding_condition": "once",
            "binding_type": "collection",
            "binding_collection_name": "actions",
            "binding_name": expression,
            "binding_name_override": "#visible"
        }]
    });
    ResolvedControl {
        name: "button".to_owned(),
        control_type: Some("button".to_owned()),
        base: None,
        unresolved_base: None,
        properties: properties
            .as_object()
            .unwrap()
            .clone()
            .into_iter()
            .collect(),
        children: Vec::new(),
        factory: None,
    }
}

#[test]
fn collection_visibility_accepts_surplus_closing_groups() {
    for expression in [
        "(not ((#text - 'header:') = (#text - '')))",
        "(not ((#text - 'header:') = (#text - ''))))",
        "(not ((#text - 'header:') = (#text - '')))))",
    ] {
        for (text, visible) in [("header:Back", true), ("Play", false)] {
            let mut data = DataSource::new();
            data.set_collection(
                "actions",
                vec![CollectionItem::new("button").with("#text", Scalar::Text(text.to_owned()))],
            );
            let bound = bind(&collection_button(expression), &data, &EmptyLibrary);
            assert_eq!(
                bound.properties.get("visible"),
                Some(&Value::Bool(visible)),
                "{expression} with {text}"
            );
        }
    }
}
