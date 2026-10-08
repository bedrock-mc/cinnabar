//! Cached declarations must follow edited templates and preserve diagnostics.

use std::sync::Arc;

use json_ui::{DataSource, EmptyLibrary, ResolvedControl, Scalar, bind_reporting};
use serde_json::json;

/// Template replacement changes binding sources without losing repeated diagnostics.
#[test]
fn edited_templates_rebind_and_cached_diagnostics_repeat() {
    let mut root = Arc::new(ResolvedControl {
        name: "label".into(),
        control_type: Some("label".into()),
        base: None,
        unresolved_base: None,
        properties: [
            ("text".into(), json!("#text")),
            (
                "bindings".into(),
                json!([{
                    "binding_type": "future_type",
                    "binding_name": "#first",
                    "binding_name_override": "#text"
                }]),
            ),
        ]
        .into(),
        children: Vec::new(),
        factory: None,
    });
    let mut data = DataSource::new();
    data.set_global("#first", Scalar::Text("first".into()));
    data.set_global("#second", Scalar::Text("second".into()));
    for _ in 0..2 {
        let (bound, diagnostics) = bind_reporting(&root, &data, &EmptyLibrary);
        assert_eq!(bound.properties["text"], json!("first"));
        assert!(diagnostics.iter().any(|note| note.contains("future_type")));
    }
    Arc::make_mut(&mut root).properties.insert(
        "bindings".into(),
        json!([{
            "binding_name": "#second", "binding_name_override": "#text"
        }]),
    );
    let (bound, diagnostics) = bind_reporting(&root, &data, &EmptyLibrary);
    assert_eq!(bound.properties["text"], json!("second"));
    assert!(diagnostics.is_empty());
}

/// Editing one clone cannot change its template or sibling, including nested JSON.
#[test]
fn shared_properties_detach_and_round_trip_as_plain_json() {
    let original = json_ui::Properties::from([("nested".into(), json!({"value":1}))]);
    let sibling = original.clone();
    let mut edited = original.clone();
    edited.get_mut("nested").unwrap()["value"] = json!(2);
    assert_eq!(original["nested"]["value"], json!(1));
    assert_eq!(sibling, original);
    let encoded = serde_json::to_value(&edited).unwrap();
    assert_eq!(encoded, json!({"nested":{"value":2}}));
    assert_eq!(
        serde_json::from_value::<json_ui::Properties>(encoded).unwrap(),
        edited
    );
}
