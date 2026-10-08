use json_ui::Catalog;
use serde_json::json;

const PATH: &str = "custom/hud.uidx";
const PARTIAL: &[u8] = br#"{
    "namespace":"custom",
    "root":{"type":"panel","controls":[{"retained":{"type":"label","text":"Kept"}},]},
    "unread":{"type":"label","text":"Not read"}
}"#;

fn catalog() -> Catalog {
    Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        ("ui/_ui_defs.json", br#"{"ui_defs":[]}"#.as_slice()),
    ])
    .unwrap()
}

fn first(catalog: &mut Catalog, bytes: &[u8]) {
    catalog.apply_pack([
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["custom/hud.uidx"]}"#.as_slice(),
        ),
        (PATH, bytes),
    ]);
}

#[test]
fn partial_document_keeps_first_file_output_before_nested_array_error() {
    let mut catalog = catalog();
    first(&mut catalog, PARTIAL);
    let root = catalog
        .lookup("custom", "root")
        .expect("retained first-file output");
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].props["text"], json!("Kept"));
    assert!(catalog.lookup("custom", "unread").is_none());
    assert!(
        catalog
            .diagnostics()
            .iter()
            .any(|note| note.contains("parse error"))
    );
}

#[test]
fn partial_document_later_malformed_overlay_does_not_replace_valid_file() {
    let mut catalog = catalog();
    first(
        &mut catalog,
        br#"{"namespace":"custom","root":{"type":"label","text":"Original"}}"#,
    );
    catalog.apply_pack([(PATH, PARTIAL)]);
    assert_eq!(
        catalog.lookup("custom", "root").unwrap().props["text"],
        json!("Original")
    );
    assert!(
        catalog
            .lookup("custom", "root")
            .unwrap()
            .children
            .is_empty()
    );
}

#[test]
fn partial_document_failed_first_file_does_not_admit_later_overlays() {
    let mut catalog = catalog();
    first(&mut catalog, PARTIAL);
    catalog.apply_pack([(
        PATH,
        br#"{"root":{"text":"Replacement"},"unread":{"type":"label"}}"#.as_slice(),
    )]);
    assert_eq!(
        catalog.lookup("custom", "root").unwrap().children[0].props["text"],
        json!("Kept")
    );
    assert!(catalog.lookup("custom", "unread").is_none());
}

#[test]
fn partial_document_failure_is_local_to_its_resource_path() {
    let mut catalog = catalog();
    first(&mut catalog, PARTIAL);
    catalog.apply_pack([
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["custom/other.uidx"]}"#.as_slice(),
        ),
        (
            "custom/other.uidx",
            br#"{"namespace":"custom","other":{"type":"label","text":"Other"}}"#.as_slice(),
        ),
    ]);
    assert_eq!(
        catalog.lookup("custom", "other").unwrap().props["text"],
        json!("Other")
    );
}

#[test]
fn partial_document_base_catalog_keeps_complete_members_before_error() {
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["custom/hud.uidx"]}"#.as_slice(),
        ),
        (PATH, PARTIAL),
    ])
    .unwrap();
    assert_eq!(
        catalog.lookup("custom", "root").unwrap().children[0].props["text"],
        json!("Kept")
    );
    assert!(catalog.lookup("custom", "unread").is_none());
}

#[test]
fn partial_document_null_first_resource_still_makes_next_file_an_overlay() {
    let mut catalog = catalog();
    first(&mut catalog, b"null");
    catalog.apply_pack([(PATH, PARTIAL)]);
    assert!(catalog.lookup("custom", "root").is_none());
    catalog.apply_pack([(
        PATH,
        br#"{"namespace":"custom","root":{"type":"label","text":"Valid"}}"#.as_slice(),
    )]);
    assert_eq!(
        catalog.lookup("custom", "root").unwrap().props["text"],
        json!("Valid")
    );
}
