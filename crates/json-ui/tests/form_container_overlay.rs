use json_ui::{
    ActionForm, Catalog, CatalogLibrary, Context, CustomForm, Draw, FormModel, LayoutEnv,
    ModalForm, Scalar, TextMeasure, TextureMeta, TextureSource, ViewState, bind, form_data_source,
    render_bound_gated, render_form,
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

/// A server pack can reuse the shared container overlay inside its form content.
fn catalog() -> Catalog {
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/test_form.json",
        &json!({
            "namespace": "server_form",
            "third_party_server_screen": {"$screen_content": "server_form.content"},
            "content": {"type": "panel", "size": [200, 100], "controls": [
                {"title": {"type": "label", "text": "#title_text", "size": [200, 10]}},
                {"hover": {"type": "custom", "renderer": "hover_text_renderer",
                    "size": [20, 20], "#hover_text": "Angler Pottery Sherd"}},
                {"container_overlay": {"type": "panel", "size": [200, 100],
                    "bindings": [{"binding_name": "#is_container_screen",
                        "binding_name_override": "#visible"}],
                    "controls": [{"bundle_hint": {"type": "label", "size": [100, 10],
                        "text": "Bundle contents"}}]}}
            ]}
        })
        .to_string(),
    );
    catalog
}

#[test]
fn server_forms_hide_container_overlays_and_preserve_normal_hover_text() {
    let catalog = catalog();
    let context = Context::desktop();
    let env = LayoutEnv {
        text: &Metrics,
        textures: &Metrics,
    };
    for model in [
        FormModel::Action(ActionForm {
            title: "Choose an item".into(),
            ..Default::default()
        }),
        FormModel::Custom(CustomForm {
            icon: None,
            title: "Choose an item".into(),
            ..Default::default()
        }),
    ] {
        let rendered = render_form(&model, &catalog, &context, [200.0, 100.0], &env).unwrap();
        assert!(rendered.nodes.iter().any(|node| matches!(&node.draw,
            Draw::Text { text, .. } if text == "Choose an item")));
        assert!(rendered.nodes.iter().any(|node| matches!(&node.draw,
            Draw::Custom { renderer, data } if renderer == "hover_text_renderer"
                && data.get("#hover_text") == Some(&json!("Angler Pottery Sherd")))));
        assert!(
            !rendered.nodes.iter().any(|node| matches!(&node.draw,
            Draw::Text { text, .. } if text == "Bundle contents")),
            "a form must not paint inherited container-only overlays"
        );
    }
    let data = form_data_source(&FormModel::Modal(ModalForm::default()));
    let root = json_ui::resolve(&catalog, "server_form.content", &context)
        .control
        .unwrap();
    let library = CatalogLibrary {
        catalog: &catalog,
        context: &context,
    };
    let mut data = data;
    data.set_global("#is_container_screen", Scalar::Bool(true));
    let bound = bind(&root, &data, &library);
    let rendered = render_bound_gated(
        bound,
        [200.0, 100.0],
        &env,
        &ViewState::default(),
        &mut Default::default(),
    );
    assert!(
        rendered.nodes.iter().any(|node| matches!(&node.draw,
        Draw::Text { text, .. } if text == "Bundle contents")),
        "the same pack overlay remains available to container controllers"
    );
}
