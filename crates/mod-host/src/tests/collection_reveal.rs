use super::*;
use serde_json::json;
use ui::mod_panel::{Panel, PanelBundle};

/// An admitted native collection with immutable items and no control authority of its own.
fn panel() -> Panel {
    serde_json::from_value(
        json!({"title":"Collection", "toggle_key":"F10", "dark":true,
        "controls":[], "surface":{"screen":"list.root", "document":json!({
            "namespace":"list", "root":{"type":"panel", "controls":[
                {"tiles":{"type":"panel", "cinnabar_collection":"modules"}}]}}).to_string(),
        "collections":{"modules":{"definition":{"columns":1,"viewport":[108,80],
            "template":{"screen":"tile.root", "document":json!({"namespace":"tile",
                "root":{"type":"panel", "size":[100,40]}}).to_string()},
            "items":vec![json!({});33]}, "order":(0..33).collect::<Vec<_>>()}}}}),
    )
    .unwrap()
}

/// The existing string import carries the additive JSON field through a real component call.
#[test]
fn collection_reveal_round_trips_through_wit_update_content() {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (namespace, version) = package.split_once('@').unwrap();
    let patch = json!({"collection_reveal":{"collection":"modules", "item":12}}).to_string();
    let bytes = patch
        .bytes()
        .map(|byte| format!("\\{byte:02x}"))
        .collect::<String>();
    let source = fixture("i32.const 1024 i32.const PATCH_LENGTH i32.const 256 call $update i32.const 256 i32.load if unreachable end", "Hello")
        .replacen("(component", &format!("(component (import \"{namespace}/panel@{version}\" (instance $panel (export \"update-content\" (func (param \"json\" string) (result (result (error string))))))) (alias export $panel \"update-content\" (func $update-content))"), 1)
        .replace("(core func $lower-label", "(core func $lower-update (canon lower (func $update-content) (memory $memory) (realloc $realloc))) (core func $lower-label")
        .replace("(import \"host\" \"time\"", "(import \"host\" \"update\" (func $update (param i32 i32 i32))) (import \"host\" \"time\"")
        .replace("(export \"time\" (func $lower-time))", "(export \"update\" (func $lower-update)) (export \"time\" (func $lower-time))")
        .replace("(data (i32.const 0)", &format!("(data (i32.const 1024) \"{bytes}\") (data (i32.const 0)"))
        .replace("PATCH_LENGTH", &patch.len().to_string());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collection.wat");
    std::fs::write(&path, source).unwrap();
    let mut host = ModHost::load_with_grants(
        &path,
        ModGrants {
            controls: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.set_panel_bundle(
        &serde_json::to_string(&PanelBundle {
            panel: panel(),
            surfaces: Default::default(),
            control_schemas: Default::default(),
        })
        .unwrap(),
    )
    .unwrap();
    host.frame(false).unwrap();
    let first = host.panel().unwrap().clone();
    let reveal = first.surface.as_ref().unwrap().collections["modules"]
        .reveal
        .as_ref()
        .unwrap();
    assert_eq!(reveal.collection, "modules");
    assert_eq!(reveal.item, 12);
    host.frame(false).unwrap();
    assert_ne!(host.panel(), Some(&first));
}
