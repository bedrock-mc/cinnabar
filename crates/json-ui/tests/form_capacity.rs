use json_ui::{ActionElement, ActionForm, Catalog, Context, FormButton, FormModel, bind_form};
use serde_json::json;

fn model(count: usize) -> FormModel {
    FormModel::Action(ActionForm {
        elements: (0..count)
            .map(|index| {
                ActionElement::Button(FormButton {
                    text: format!("Choice {index}"),
                    image: None,
                })
            })
            .collect(),
        ..Default::default()
    })
}

fn catalog(descendants: usize) -> Catalog {
    let mut catalog = Catalog::default();
    let controls: Vec<_> = (0..descendants)
        .map(|index| json!({format!("child_{index}"): {"type":"panel"}}))
        .collect();
    catalog.overlay_text("ui/server_form.json", &json!({
        "namespace":"server_form",
        "long_form": {
            "type":"stack_panel", "collection_name":"form_buttons",
            "factory":{"name":"buttons", "control_ids":{"button":"server_form.button"}},
            "bindings":[{"binding_name":"#form_button_contents", "binding_name_override":"#collection_length"}]
        },
        "button":{"type":"button", "controls":controls}
    }).to_string());
    catalog
}

#[test]
fn over_capacity_forms_are_rejected_instead_of_publishing_a_shorter_menu() {
    assert!(bind_form(&model(5_000), &catalog(0), &Context::desktop()).is_none());
}

#[test]
fn form_capacity_counts_every_descendant_in_each_button_hierarchy() {
    assert!(bind_form(&model(300), &catalog(700), &Context::desktop()).is_none());
}

#[test]
fn complete_menus_above_256_keep_the_final_collection_cursor() {
    for count in [257usize, 300] {
        let bound = bind_form(&model(count), &catalog(2), &Context::desktop()).unwrap();
        assert_eq!(bound.children.len(), count);
        let last = &bound.children[count - 1];
        assert_eq!(
            last.properties.get("collection_index"),
            Some(&json!(count - 1))
        );
    }
}
