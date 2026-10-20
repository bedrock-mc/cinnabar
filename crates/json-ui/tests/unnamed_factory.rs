use std::sync::Arc;

use json_ui::{
    BindState, Catalog, CatalogLibrary, Context, DataSource, Scalar, bind_incremental, resolve,
};
use serde_json::json;

#[test]
fn a_stack_panel_factory_obeys_its_bound_count_without_a_named_collection() {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/gated.json", &json!({
        "namespace": "gated",
        "screen": {
            "type": "stack_panel",
            "factory": {"name": "selected", "control_name": "gated.child"},
            "bindings": [
                {"binding_type": "global", "binding_name": "#shown"},
                {"binding_type": "view", "source_property_name": "#shown", "target_property_name": "#collection_length"}
            ]
        },
        "child": {"type": "label", "text": "mounted"}
    }).to_string());
    let context = Context::desktop();
    let library = CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let root = Arc::new(resolve(&catalog, "gated.screen", &context).control.unwrap());
    let mut state = BindState::new();
    for (value, count) in [
        (Scalar::Int(2), 2),
        (Scalar::Bool(false), 0),
        (Scalar::Bool(true), 1),
        (Scalar::Int(0), 0),
    ] {
        let mut data = DataSource::new();
        data.set_global("#shown", value);
        let data = Arc::new(data);
        for _ in 0..3 {
            let tree = bind_incremental(&root, &data, &library, &mut state);
            assert_eq!(tree.children.len(), count);
            assert!(
                tree.children
                    .iter()
                    .all(|child| child.properties.get("text") == Some(&json!("mounted")))
            );
        }
    }
}
