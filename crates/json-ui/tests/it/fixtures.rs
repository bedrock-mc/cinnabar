//! Fixture tests that resolve real vanilla `ui/*.json` from `.local/` and assert
//! the concrete tree. They explain missing fixtures and skip until the pack is fetched;
//! they protect inheritance, substitution,
//! `ignored` removal, and factory recording.

use crate::support;

use std::path::PathBuf;

use json_ui::{Catalog, Context, ControlRef, ResolvedControl, resolve};
use serde_json::json;

fn ui_dir() -> Option<PathBuf> {
    let dir = support::vanilla_pack().join("ui");
    dir.is_dir().then_some(dir)
}

fn catalog() -> Option<Catalog> {
    let dir = ui_dir()?;
    Some(Catalog::load_dir(&dir).expect("index files load"))
}

fn prop<'a>(control: &'a ResolvedControl, key: &str) -> &'a serde_json::Value {
    control
        .properties
        .get(key)
        .unwrap_or_else(|| panic!("{} missing property {key}", control.name))
}

/// Both forms inherit `common_dialogs.main_panel_no_buttons`; check the shared
/// dialog skeleton, then let each caller check its own child control.
fn assert_dialog_skeleton(root: &ResolvedControl, base_form: &str) {
    assert_eq!(root.name, base_form);
    assert_eq!(
        root.control_type.as_deref(),
        Some("panel"),
        "type inherited from base"
    );
    assert_eq!(
        root.base,
        Some(ControlRef::new("common_dialogs", "main_panel_no_buttons"))
    );
    assert_eq!(
        prop(root, "size"),
        &json!([225, 200]),
        "child override wins"
    );
    assert_eq!(
        prop(root, "anchor_from"),
        &json!("center"),
        "base-only property kept"
    );

    let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["common_panel", "title_label", "panel_indent"]);

    // `$custom_background` default flows in as the background control's base ref.
    let common_panel = root.child("common_panel").unwrap();
    assert_eq!(common_panel.control_type.as_deref(), Some("panel"));
    let bg = common_panel
        .child("bg_image")
        .expect("bg_image from $dialog_background");
    assert_eq!(
        bg.base,
        Some(ControlRef::new("common", "dialog_background_hollow_3"))
    );

    // title_label keeps the standard label and drops the ignored custom slot.
    let title = root.child("title_label").unwrap();
    assert_eq!(
        title.base,
        Some(ControlRef::new("common_dialogs", "title_label"))
    );
    assert_eq!(
        title.children.len(),
        1,
        "ignored custom title control dropped"
    );
    let label = &title.children[0];
    assert_eq!(label.name, "common_dialogs_0");
    assert_eq!(label.control_type.as_deref(), Some("label"));
    assert_eq!(
        label.base,
        Some(ControlRef::new("common_dialogs", "standard_title_label"))
    );
    // global substitution: $title_text_color -> [0.3, 0.3, 0.3].
    assert_eq!(prop(label, "color"), &json!([0.3, 0.3, 0.3]));
    // $var propagation through inheritance: long/custom set $text_name = "#title_text".
    assert_eq!(prop(label, "text"), &json!("#title_text"));

    // panel_indent size comes from the $panel_indent_size default, left symbolic.
    let indent = root.child("panel_indent").unwrap();
    assert_eq!(prop(indent, "size"), &json!(["100% - 16px", "100% - 31px"]));
}

#[test]
fn long_form_resolves_full_dialog_tree() {
    let Some(catalog) = catalog() else {
        return;
    };
    let control = resolve(&catalog, "server_form.long_form", &Context::desktop())
        .control
        .expect("long_form resolves");
    assert_dialog_skeleton(&control, "long_form");

    // $child_control routes the dialog body to the long-form scrolling stack.
    let inside = control
        .child("panel_indent")
        .unwrap()
        .child("inside_header_panel")
        .unwrap();
    assert_eq!(
        inside.base,
        Some(ControlRef::new("server_form", "long_form_panel"))
    );
    assert_eq!(inside.control_type.as_deref(), Some("stack_panel"));
}

#[test]
fn custom_form_resolves_full_dialog_tree() {
    let Some(catalog) = catalog() else {
        return;
    };
    let control = resolve(&catalog, "server_form.custom_form", &Context::desktop())
        .control
        .expect("custom_form resolves");
    assert_dialog_skeleton(&control, "custom_form");

    // custom_form routes to a scrolling panel directly.
    let inside = control
        .child("panel_indent")
        .unwrap()
        .child("inside_header_panel")
        .unwrap();
    assert_eq!(
        inside.base,
        Some(ControlRef::new("server_form", "custom_form_panel"))
    );
    assert_eq!(inside.control_type.as_deref(), Some("panel"));
}

#[test]
fn main_panel_no_buttons_standalone_leaves_child_control_unresolved() {
    let Some(catalog) = catalog() else {
        return;
    };
    let control = resolve(
        &catalog,
        "common_dialogs.main_panel_no_buttons",
        &Context::desktop(),
    )
    .control
    .expect("main_panel_no_buttons resolves");
    assert_eq!(control.name, "main_panel_no_buttons");
    assert_eq!(control.control_type.as_deref(), Some("panel"));
    assert_eq!(control.base, None, "root template has no base");
    assert_eq!(prop(&control, "anchor_to"), &json!("center"));

    // With no $text_name supplied, the title label's default "" is substituted.
    let label = control
        .child("title_label")
        .unwrap()
        .child("common_dialogs_0")
        .unwrap();
    assert_eq!(prop(label, "text"), &json!(""));
    assert_eq!(control.child("title_label").unwrap().children.len(), 1);

    // $child_control is unbound here, so the body slot is flagged, not fabricated.
    let inside = control
        .child("panel_indent")
        .unwrap()
        .child("inside_header_panel")
        .unwrap();
    assert_eq!(inside.unresolved_base.as_deref(), Some("$child_control"));
    assert_eq!(inside.base, None);
    assert_eq!(inside.control_type, None);
}

#[test]
fn form_factory_records_control_ids() {
    let Some(catalog) = catalog() else {
        return;
    };
    let content = resolve(
        &catalog,
        "server_form.main_screen_content",
        &Context::desktop(),
    )
    .control
    .expect("main_screen_content resolves");
    let factory_node = content
        .child("server_form_factory")
        .expect("factory child present");
    assert_eq!(factory_node.control_type.as_deref(), Some("factory"));
    let factory = factory_node.factory.as_ref().expect("factory recorded");
    assert_eq!(
        factory.control_ids.get("long_form"),
        Some(&ControlRef::new("server_form", "long_form"))
    );
    assert_eq!(
        factory.control_ids.get("custom_form"),
        Some(&ControlRef::new("server_form", "custom_form"))
    );
    // control_ids are consumed into the factory, not left as a raw property.
    assert!(!factory_node.properties.contains_key("control_ids"));
}

#[test]
fn dynamic_buttons_panel_records_named_factory_and_collection() {
    let Some(catalog) = catalog() else {
        return;
    };
    let panel = resolve(
        &catalog,
        "server_form.long_form_dynamic_buttons_panel",
        &Context::desktop(),
    )
    .control
    .expect("dynamic buttons panel resolves");
    let factory = panel.factory.as_ref().expect("factory property recorded");
    assert_eq!(factory.name.as_deref(), Some("buttons"));
    assert_eq!(
        factory.control_ids.get("button"),
        Some(&ControlRef::new("server_form", "dynamic_button"))
    );
    assert_eq!(
        factory.control_ids.get("divider"),
        Some(&ControlRef::new(
            "settings_common",
            "option_group_section_divider"
        ))
    );
    assert_eq!(prop(&panel, "collection_name"), &json!("form_buttons"));
}

#[test]
fn generated_contents_records_custom_form_factory() {
    let Some(catalog) = catalog() else {
        return;
    };
    let generated = resolve(
        &catalog,
        "server_form.generated_contents",
        &Context::desktop(),
    )
    .control
    .expect("generated_contents resolves");
    let factory = generated.factory.as_ref().expect("factory recorded");
    assert_eq!(factory.name.as_deref(), Some("buttons"));
    for role in ["label", "toggle", "slider", "dropdown", "input", "header"] {
        assert!(
            factory.control_ids.contains_key(role),
            "missing factory role {role}"
        );
    }
    assert_eq!(prop(&generated, "collection_name"), &json!("custom_form"));
}

#[test]
fn catalog_loads_every_namespace_without_parse_errors() {
    let Some(catalog) = catalog() else {
        return;
    };
    assert!(
        catalog.namespace_count() > 150,
        "expected the full pack of namespaces"
    );
    let parse_failures: Vec<&String> = catalog
        .diagnostics()
        .iter()
        .filter(|line| line.contains("parse error"))
        .collect();
    assert!(
        parse_failures.is_empty(),
        "unexpected parse failures: {parse_failures:?}"
    );
}
