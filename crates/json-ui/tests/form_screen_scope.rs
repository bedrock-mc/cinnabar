use json_ui::{
    ActionForm, Catalog, Context, CustomForm, Draw, FormModel, LayoutEnv, TextMeasure, TextureMeta,
    TextureSource, render_form,
};

const PACK: &str = include_str!("fixtures/server_form_scope.json");

struct Metrics;

impl TextMeasure for Metrics {
    /// Gives fixture labels deterministic dimensions without a font carrier.
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}

impl TextureSource for Metrics {
    /// The original fixture has no texture assets.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// Layers the original server fixture over a catalog with an indexed form document.
fn catalog(pack: &str) -> Catalog {
    let mut catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/server_form.json"]}"#.as_slice(),
        ),
        (
            "ui/server_form.json",
            br#"{"namespace":"server_form"}"#.as_slice(),
        ),
    ])
    .unwrap();
    catalog.apply_pack([("ui/server_form.json", pack.as_bytes())]);
    catalog
}

/// Returns visible labels after screen routing, binding, layout and emission.
fn labels(catalog: &Catalog, model: &FormModel, context: &Context) -> Vec<String> {
    render_form(
        model,
        catalog,
        context,
        [320.0, 180.0],
        &LayoutEnv {
            text: &Metrics,
            textures: &Metrics,
        },
    )
    .expect("form resolves")
    .nodes
    .iter()
    .filter_map(|node| match &node.draw {
        Draw::Text { text, .. } => Some(text.clone()),
        _ => None,
    })
    .collect()
}

#[test]
fn aliased_screen_content_selects_the_title_driven_server_panel() {
    let catalog = catalog(PACK);
    for (title, panel) in [("Market", "Buy supplies"), ("Jobs", "Ordinary menu")] {
        for model in [
            FormModel::Action(ActionForm {
                title: title.into(),
                ..Default::default()
            }),
            FormModel::Custom(CustomForm {
                title: title.into(),
                ..Default::default()
            }),
        ] {
            assert_eq!(
                labels(&catalog, &model, &Context::desktop()),
                ["Desktop market", panel]
            );
        }
    }
}

#[test]
fn literal_screen_content_keeps_the_screens_selected_variable_scope() {
    let pack = PACK.replace(
        "\"$screen_content\": \"$form_content\"",
        "\"$screen_content\": \"server_form.scoped_content\"",
    );
    assert_eq!(
        labels(
            &catalog(&pack),
            &FormModel::Action(ActionForm {
                title: "Market".into(),
                ..Default::default()
            }),
            &Context::desktop()
        ),
        ["Desktop market", "Buy supplies"]
    );
}
