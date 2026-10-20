use json_ui::{Catalog, Context, resolve};

#[test]
fn a_pack_rebase_replaces_the_previous_definition_body() {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/form.json", r##"{
        "namespace":"forms",
        "generated":{"type":"stack_panel","collection_name":"fields","factory":{"name":"fields","control_ids":{"old":"forms.old"}}},
        "router":{"type":"collection_panel","collection_name":"route","property_bag":{"#collection_length":["active"]},"factory":{"name":"route","control_ids":{"active":"forms.current"}}},
        "current":{"type":"label","text":"Current fields"},
        "old":{"type":"label","text":"Old fields"}
    }"##);
    catalog.overlay_text(
        "ui/form.json",
        r##"{
        "namespace":"forms","generated@router":{"size":["100%","100%c"]}
    }"##,
    );
    let result = resolve(&catalog, "forms.generated", &Context::empty());
    let control = result.control.unwrap();
    assert_eq!(control.control_type.as_deref(), Some("collection_panel"));
    assert_eq!(
        control.properties.get("collection_name"),
        Some(&serde_json::json!("route"))
    );
    let ids = &control.factory.unwrap().control_ids;
    assert!(ids.contains_key("active"));
    assert!(!ids.contains_key("old"));
}
