use json_ui::{
    ActionElement, ActionForm, Catalog, Context, DataSource, Draw, EmptyLibrary, FormButton,
    FormModel, LayoutEnv, Scalar, TextMeasure, TextureMeta, TextureSource, bind, render_form,
};
use serde_json::json;

const BODY: &str = "Match instructions";

fn catalog() -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/server_form.json",
        &json!({
            "namespace": "server_form",
            "third_party_server_screen": {
                "type": "screen",
                "$screen_content": "server_form.main_screen_content"
            },
            "main_screen_content": {
                "type": "panel", "size": [200, 100],
                "controls": [{"server_form_factory": {
                    "type": "factory",
                    "control_ids": {"long_form": "@server_form.long_form"}
                }}]
            },
            "long_form": {
                "type": "stack_panel", "size": [200, 100], "orientation": "vertical",
                "controls": [
                    {"title": {
                        "type": "label", "size": [200, 20], "text": "#title_text"
                    }},
                    {"description": {
                        "type": "label", "size": [200, 20], "text": "#form_text"
                    }},
                    {"named_description": {
                        "type": "label", "size": [200, 20], "text": "#resolved_body",
                        "property_bag": {"#resolved_body": "missing"},
                        "bindings": [{
                            "binding_type": "view",
                            "source_control_name": "long_form",
                            "source_property_name": "#form_text",
                            "target_property_name": "#resolved_body"
                        }]
                    }},
                    {"play": {
                        "type": "button", "size": [100, 20], "visible": false,
                        "property_bag": {"#visible": false},
                        "bindings": [{
                            "binding_type": "view",
                            "source_control_name": "long_form",
                            "source_property_name": format!("(#form_text = '{BODY}')"),
                            "target_property_name": "#visible"
                        }],
                        "button_mappings": [{
                            "from_button_id": "button.menu_select",
                            "to_button_id": "button.form_button_click",
                            "mapping_type": "pressed"
                        }]
                    }},
                    {"role_preview": {
                        "type": "stack_panel", "size": [200, 80], "orientation": "vertical",
                        "collection_name": "preview",
                        "factory": {"name": "role_preview", "control_ids": {
                            "button": "server_form.preview_button",
                            "label": "server_form.preview_label",
                            "header": "server_form.preview_header",
                            "divider": "server_form.preview_divider"
                        }},
                        "bindings": [{
                            "binding_name": "#form_button_contents",
                            "binding_name_override": "#collection_length"
                        }]
                    }}
                ]
            },
            "preview_button": {"type": "label", "size": [200, 20], "text": "button"},
            "preview_label": {"type": "label", "size": [200, 20], "text": "label"},
            "preview_header": {"type": "label", "size": [200, 20], "text": "header"},
            "preview_divider": {"type": "label", "size": [200, 20], "text": "divider"}
        })
        .to_string(),
    );
    catalog
}

struct Metrics;
impl TextMeasure for Metrics {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64, 10.0]
    }
}
impl TextureSource for Metrics {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

#[test]
fn long_form_creation_body_reaches_named_source_text_and_visibility() {
    let model = FormModel::Action(ActionForm {
        title: "Arena".into(),
        body: BODY.into(),
        ..ActionForm::default()
    });
    let render = render_form(
        &model,
        &catalog(),
        &Context::desktop(),
        [200.0, 100.0],
        &LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        },
    )
    .expect("form factory resolves");
    let drawn = |name: &str| {
        render.nodes.iter().find_map(|node| match &node.draw {
            Draw::Text { text, .. } if node.name == name => Some(text.as_str()),
            _ => None,
        })
    };
    assert_eq!(drawn("title"), Some("Arena"));
    let has_play = render
        .hits
        .iter()
        .any(|hit| hit.pressed.as_deref() == Some("button.form_button_click"));
    assert_eq!(
        (drawn("description"), drawn("named_description"), has_play),
        (Some(BODY), Some(BODY), true)
    );
}

#[test]
fn named_source_views_do_not_substitute_unwritten_controller_globals() {
    let root = json_ui::resolve(&catalog(), "server_form.long_form", &Context::desktop())
        .control
        .unwrap();
    let mut data = DataSource::new();
    data.set_global("#form_text", Scalar::Text(BODY.into()));
    let bound = bind(&root, &data, &EmptyLibrary);
    let description = bound
        .find(&|control| control.name == "named_description")
        .unwrap();
    let play = bound.find(&|control| control.name == "play").unwrap();
    assert_eq!(description.properties.get("text"), Some(&json!("missing")));
    assert_eq!(play.properties.get("visible"), Some(&json!(false)));
}

#[test]
fn form_button_contents_selects_ordered_factory_roles_without_a_supplied_collection() {
    let model = FormModel::Action(ActionForm {
        elements: vec![
            ActionElement::Button(FormButton::default()),
            ActionElement::Label("Label".into()),
            ActionElement::Header("Header".into()),
            ActionElement::Divider,
        ],
        ..ActionForm::default()
    });
    let render = render_form(
        &model,
        &catalog(),
        &Context::desktop(),
        [200.0, 200.0],
        &LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        },
    )
    .expect("form factory resolves");
    let roles: Vec<_> = render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } if node.name.starts_with("preview_") => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(roles, ["button", "label", "header", "divider"]);
}
