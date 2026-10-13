//! A capped collection factory keeps the newest form row and its response cursor.

use json_ui::{
    ActionElement, ActionForm, Catalog, Context, FormButton, FormModel, LayoutEnv, TextMeasure,
    TextureMeta, TextureSource, render_form,
};
use serde_json::json;

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

/// Builds the original Back factory with caller-selected templates and subtree workload.
fn catalog(factory: serde_json::Value, descendants: Vec<serde_json::Value>) -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/server_form.json",
        &json!({
            "namespace": "server_form",
            "third_party_server_screen": {
                "type": "screen", "$screen_content": "server_form.content"
            },
            "content": {"type": "panel", "controls": [{
                "server_form_factory": {
                    "type": "factory", "control_ids": {"long_form": "server_form.long_form"}
                }
            }]},
            "long_form": {"type": "panel", "controls": [{"back": {
                "type": "collection_panel", "collection_name": "form_buttons",
                "factory": factory,
                "bindings": [{"binding_name": "#form_button_contents", "binding_name_override": "#collection_length"}]
            }}]},
            "back_button": {
                "type": "button", "size": [24, 24],
                "bindings": [
                    {"binding_type": "collection_details", "binding_collection_name": "form_buttons"},
                    {"binding_type": "collection", "binding_collection_name": "form_buttons", "binding_name": "#form_button_text"},
                    {"binding_type": "view", "source_property_name": "(#form_button_text = 'Back')", "target_property_name": "#visible"}
                ],
                "controls": descendants,
                "button_mappings": [{"from_button_id": "button.menu_select", "mapping_type": "pressed", "to_button_id": "button.form_button_click"}]
            }
        }).to_string(),
    );
    catalog
}

#[test]
fn capped_form_factory_preserves_the_last_button_response_index() {
    let catalog = catalog(
        json!({"name": "back", "control_name": "server_form.back_button", "max_children_size": 1}),
        Vec::new(),
    );
    let form = FormModel::Action(ActionForm {
        title: "Supply shop".into(),
        elements: ["Blocks", "Tools", "Back"]
            .into_iter()
            .map(|text| {
                ActionElement::Button(FormButton {
                    text: text.into(),
                    image: None,
                })
            })
            .collect(),
        ..Default::default()
    });
    let rendered = render_form(
        &form,
        &catalog,
        &Context::desktop(),
        [320.0, 180.0],
        &LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        },
    )
    .expect("original form fixture resolves");
    let back = rendered
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("button.form_button_click"))
        .expect("the final Back row must remain clickable");
    assert_eq!(back.collection.as_deref(), Some("form_buttons"));
    assert_eq!(back.collection_index, Some(2));
}

#[test]
fn discarded_factory_rows_do_not_exhaust_the_rendered_screen_budget() {
    let descendants = (0..512).map(|index| json!({format!("label_{index}"): {"type":"label", "text":"Supply", "size":[24,10]}})).collect();
    let catalog = catalog(
        json!({"name":"back", "control_name":"server_form.back_button", "max_children_size":1}),
        descendants,
    );
    let mut elements = vec![
        ActionElement::Button(FormButton {
            text: "Supply".into(),
            image: None
        });
        511
    ];
    elements.push(ActionElement::Button(FormButton {
        text: "Back".into(),
        image: None,
    }));
    let rendered = render_form(
        &FormModel::Action(ActionForm {
            title: "Supply shop".into(),
            elements,
            ..Default::default()
        }),
        &catalog,
        &Context::desktop(),
        [320.0, 180.0],
        &LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        },
    )
    .expect("discarded rows must leave the small surviving screen renderable");
    let back = rendered
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("button.form_button_click"))
        .expect("the final Back row remains clickable");
    assert_eq!(back.collection_index, Some(511));
}

#[test]
fn an_unresolved_trailing_role_does_not_evict_the_last_valid_button() {
    let catalog = catalog(
        json!({"name":"back", "control_ids":{"button":"server_form.back_button"}, "max_children_size":1}),
        Vec::new(),
    );
    let form = FormModel::Action(ActionForm {
        title: "Supply shop".into(),
        elements: vec![
            ActionElement::Button(FormButton {
                text: "Back".into(),
                image: None,
            }),
            ActionElement::Label("Tail has no template".into()),
        ],
        ..Default::default()
    });
    let rendered = render_form(
        &form,
        &catalog,
        &Context::desktop(),
        [320.0, 180.0],
        &LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        },
    )
    .unwrap();
    let back = rendered
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("button.form_button_click"))
        .expect("unresolved roles do not create children to evict the preceding Back button");
    assert_eq!(back.collection_index, Some(0));
}
