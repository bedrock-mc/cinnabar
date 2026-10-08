//! Definition indexes select JSON-UI documents independently of file extension.

use json_ui::{Catalog, Context};
use serde_json::json;

/// Builds the base inventory catalog that the custom pack overlays.
fn base() -> Catalog {
    Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/inventory.json"]}"#.as_slice(),
        ),
        (
            "ui/inventory.json",
            br#"{"namespace":"crafting","screen":{"type":"panel"}}"#.as_slice(),
        ),
    ])
    .unwrap()
}

#[test]
fn declared_custom_extension_resolves_inventory_content() {
    let mut catalog = base();
    let files = [
        ("ui/_ui_defs.json", br#"{"ui_defs":["uidx/inventory_screen.uidx"]}"#.as_slice()),
        ("uidx/inventory_screen.uidx", br#"{"namespace":"inventory_dx","content":{"type":"panel","controls":[{"slot":{"type":"image","texture":"slot","size":[16,16]}}]}}"#.as_slice()),
        ("ui/inventory.json", br#"{"screen":{"controls":[{"inventory@inventory_dx.content":{}}]}}"#.as_slice()),
    ];
    assert!(catalog.overlay_namespaces(files).contains("inventory_dx"));
    catalog.apply_pack(files);
    let root = json_ui::resolve(&catalog, "crafting.screen", &Context::empty())
        .control
        .unwrap();
    assert_eq!(
        root.children[0].children[0].properties["texture"],
        json!("slot")
    );
    assert!(catalog.diagnostics().is_empty());
}

#[test]
fn later_layer_overrides_inherited_custom_path_without_repeating_index_or_namespace() {
    let mut catalog = base();
    catalog.apply_pack([
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["custom/inventory"]}"#.as_slice(),
        ),
        (
            "custom/inventory",
            br#"{"namespace":"custom","slot":{"type":"image","texture":"old"}}"#.as_slice(),
        ),
    ]);
    let overlay = [(
        "custom/inventory",
        br#"{"slot":{"texture":"new"}}"#.as_slice(),
    )];
    assert!(catalog.overlay_namespaces(overlay).contains("custom"));
    catalog.apply_pack(overlay);
    assert_eq!(
        catalog.lookup("custom", "slot").unwrap().props["texture"],
        json!("new")
    );
}

#[test]
fn unlisted_and_malformed_documents_do_not_install_controls() {
    let mut catalog = base();
    let files = [
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["custom/broken.uidx"]}"#.as_slice(),
        ),
        ("custom/broken.uidx", b"not json".as_slice()),
        (
            "custom/unlisted.uidx",
            br#"{"namespace":"unlisted","slot":{"type":"panel"}}"#.as_slice(),
        ),
    ];
    assert!(!catalog.overlay_namespaces(files).contains("unlisted"));
    catalog.apply_pack(files);
    assert!(catalog.lookup("unlisted", "slot").is_none());
    let notes = catalog.diagnostics().join("\n");
    assert!(notes.contains("custom/broken.uidx: parse error"));
    assert!(notes.contains("custom/unlisted.uidx: not listed"));
}

#[test]
fn declared_paths_accept_comments_and_sort_duplicates() {
    assert_eq!(
        Catalog::declared_paths(
            br#"{ /* native index */ "ui_defs": ["z.uidx", "a.uidx", "z.uidx", 1] }"#
        )
        .unwrap(),
        ["a.uidx", "z.uidx"]
    );
    assert!(Catalog::declared_paths(br#"{"ui_defs":false}"#).is_err());
}

#[test]
fn malformed_index_does_not_admit_custom_paths() {
    let mut catalog = base();
    let files = [
        ("ui/_ui_defs.json", b"broken index".as_slice()),
        (
            "custom/screen.uidx",
            br#"{"namespace":"custom","slot":{"type":"panel"}}"#.as_slice(),
        ),
    ];
    assert!(!catalog.overlay_namespaces(files).contains("custom"));
    catalog.apply_pack(files);
    assert!(catalog.lookup("custom", "slot").is_none());
    assert!(!catalog.diagnostics().is_empty());
}

#[test]
fn overlay_control_paths_follow_accepted_indexes_and_inherited_namespaces() {
    let catalog = base();
    let files = [
        (
            "ui/inventory.json",
            br#"{"screen/label":{"font_size":"large"},"broken":7}"#.as_slice(),
        ),
        (
            "ui/unlisted.json",
            br#"{"namespace":"unlisted","title":{}}"#.as_slice(),
        ),
    ];
    let paths = catalog.overlay_controls(files);
    assert_eq!(
        paths,
        [json_ui::ControlRef::new("crafting", "screen/label")]
            .into_iter()
            .collect()
    );
    assert!(
        catalog
            .overlay_controls([(
                "ui/inventory.json",
                br#"{"screen":{},"broken":[1,]}"#.as_slice()
            )])
            .is_empty()
    );
}
