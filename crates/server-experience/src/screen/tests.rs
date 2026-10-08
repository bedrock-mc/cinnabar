use super::*;

fn text(value: &str) -> Value {
    Value::Text(value.to_owned())
}

#[test]
fn values_use_the_record_leaf_encoding_plus_reals_and_arrays() {
    let row: Row = serde_json::from_str(
        r##"{"#name":{"type":"text","value":"§eStone"},"#count":{"type":"integer","value":64},
            "#shown":{"type":"bool","value":true},"#progress":{"type":"number","value":0.5},
            "#color":{"type":"numbers","value":[1,0.5,0,1]}}"##,
    )
    .unwrap();
    validate_rows(std::slice::from_ref(&row)).unwrap();
    assert_eq!(row["#name"], text("§eStone"));
    assert_eq!(row["#color"], Value::Numbers(vec![1.0, 0.5, 0.0, 1.0]));
}

#[test]
fn malformed_values_and_names_are_rejected() {
    for value in [
        Value::Number(f64::NAN),
        Value::Numbers(Vec::new()),
        Value::Numbers(vec![0.0; MAX_UI_NUMBERS + 1]),
        Value::Numbers(vec![f64::INFINITY]),
        text(&"x".repeat(MAX_WIDGET_TEXT_BYTES + 1)),
        text("bell\u{7}"),
    ] {
        assert!(value.validate().is_err(), "{value:?}");
    }
    text("two\nlines §lbold").validate().unwrap();
    for name in ["", "#", "name", "#a b", "#a/b"] {
        assert!(!binding_name(name), "{name}");
    }
    assert!(binding_name("#propagateAlpha") && binding_name("#item.count_2"));
    assert!(collection_name("inventory_items") && !collection_name("#items"));
}

#[test]
fn oversized_collections_are_rejected() {
    let row = Row::from([("#n".to_owned(), Value::Integer(1))]);
    validate_rows(&vec![row.clone(); MAX_COLLECTION_ROWS]).unwrap();
    assert!(validate_rows(&vec![row; MAX_COLLECTION_ROWS + 1]).is_err());
    let wide: Row = (0..=MAX_ROW_FIELDS)
        .map(|i| (format!("#f{i}"), Value::Bool(true)))
        .collect();
    assert!(validate_rows(&[wide]).is_err());
    let unnamed = Row::from([("count".to_owned(), Value::Integer(1))]);
    assert!(validate_rows(&[unnamed]).is_err());
}

#[test]
fn templates_declare_their_bundle_namespace_and_report_foreign_references() {
    let template = br##"{
        "namespace": "benergistics",
        "terminal@common.base_screen": {
            "$screen_content": "benergistics.grid",
            "controls": [
                {"panel@entry": {}},
                {"close@common_buttons.close_button": {}},
                {"icon@$icon_ref": {}}
            ]
        },
        "entry": {"type": "panel", "$icon_ref": "icon@common.item_renderer",
                  "anims": ["@chest.anim_open"], "$pressed_button_name": "button.menu_select"}
    }"##;
    let foreign = validate_template(template, "benergistics").unwrap();
    let pair = |namespace: &str, name: &str| (namespace.to_owned(), name.to_owned());
    assert_eq!(
        foreign,
        BTreeSet::from([
            pair("chest", "anim_open"),
            pair("common", "base_screen"),
            pair("common", "item_renderer"),
            pair("common_buttons", "close_button"),
        ])
    );
    let wrong = br#"{"namespace": "common", "x": {}}"#;
    assert!(validate_template(wrong, "benergistics").is_err());
    let missing = br#"{"x": {}}"#;
    assert!(validate_template(missing, "benergistics").is_err());
    assert!(validate_template(b"[1]", "benergistics").is_err());
    assert!(
        validate_template(
            b"{\"namespace\": \"benergistics\", // comment\n}",
            "benergistics"
        )
        .is_err()
    );
    let oversized = format!(
        r#"{{"namespace":"benergistics","x":{{"text":"{}"}}}}"#,
        "x".repeat(MAX_TEMPLATE_BYTES)
    );
    assert!(validate_template(oversized.as_bytes(), "benergistics").is_err());
}

#[test]
fn modal_state_replaces_screens_and_bounds_bound_data() {
    let mut modal = Modal::default();
    modal.open(Some("ui/terminal.json".into()));
    modal.set_collection("items".into(), vec![Row::new(); 2]);
    modal.set_value("#title".into(), text("ME Terminal"));
    let revision = modal.revision;
    modal.open(Some("ui/settings.json".into()));
    assert_eq!(modal.template.as_deref(), Some("ui/settings.json"));
    assert!(modal.revision > revision);
    assert_eq!(
        modal.collections["items"].len(),
        2,
        "data outlives a screen switch"
    );
    modal.open(None);
    assert!(modal.template.is_none());
    for i in 0..MAX_COLLECTIONS {
        modal.set_collection(format!("c{i}"), Vec::new());
    }
    assert!(modal.check().is_err());
}

#[test]
fn modal_commands_need_the_permission_and_an_indexed_template() {
    use crate::{
        manifest::{Permission, Scope},
        runtime::{Capabilities, Command, Contributions, Principal, Transaction},
    };
    let mut capabilities = Capabilities {
        scope: Scope {
            permissions: BTreeSet::from([Permission::ModalUi]),
            origins: BTreeSet::new(),
            memory_bytes: 0,
            gpu_bytes: 0,
        },
        assets: BTreeSet::from(["ui/terminal.json".to_owned(), "ui/other.json".to_owned()]),
        templates: BTreeSet::from(["ui/terminal.json".to_owned()]),
        channels: Vec::new(),
        actions: BTreeSet::new(),
        max_message_bytes: crate::policy::MAX_MESSAGE_BYTES as u32,
    };
    let open = |template: &str| Command::Screen {
        template: Some(template.to_owned()),
    };
    capabilities.validate(&open("ui/terminal.json")).unwrap();
    assert!(capabilities.validate(&open("ui/other.json")).is_err());
    let rows = Command::Collection {
        name: "items".into(),
        rows: vec![Row::from([("#name".to_owned(), text("Stone"))])],
    };
    capabilities.validate(&rows).unwrap();
    let oversized = Command::Collection {
        name: "items".into(),
        rows: vec![Row::new(); MAX_COLLECTION_ROWS + 1],
    };
    assert!(capabilities.validate(&oversized).is_err());
    let owner = Principal {
        session: "s".into(),
        bundle: "b".into(),
        generation: 1,
    };
    let transaction = Transaction {
        owner: owner.clone(),
        epoch: 1,
        commands: vec![
            open("ui/terminal.json"),
            rows,
            Command::Value {
                name: "#title".into(),
                value: text("ME Terminal"),
            },
        ],
    };
    let mut contributions = Contributions::default();
    contributions
        .apply(&transaction, &owner, 1, &capabilities)
        .unwrap();
    assert_eq!(
        contributions.modal.template.as_deref(),
        Some("ui/terminal.json")
    );
    assert_eq!(contributions.modal.values["#title"], text("ME Terminal"));
    capabilities.scope.permissions.clear();
    assert!(capabilities.validate(&open("ui/terminal.json")).is_err());
}
