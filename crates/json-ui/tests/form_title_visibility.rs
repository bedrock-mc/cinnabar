use json_ui::{
    ActionForm, Catalog, Context, Draw, FormModel, LayoutEnv, TextMeasure, TextureMeta,
    TextureSource, render_form,
};

struct Metrics;

impl TextMeasure for Metrics {
    /// Measures the original fixture without loading a font carrier.
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 10.0]
    }
}

impl TextureSource for Metrics {
    /// The original fixture uses only text.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

#[test]
fn title_selected_panel_draws_through_an_overridden_form_factory() {
    let mut catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br##"{"ui_defs":["ui/server_form.json"]}"##.as_slice(),
        ),
        (
            "ui/server_form.json",
            br##"{"namespace":"server_form",
                "third_party_server_screen":{"type":"screen","$screen_content":"server_form.content"},
                "content":{"type":"panel","size":[0,0],"controls":[
                    {"server_form_factory":{"type":"factory","control_ids":{"long_form":"server_form.long_form"}}}
                ]},
                "long_form":{"type":"label","text":"Default","size":[100,20]}
            }"##.as_slice(),
        ),
    ])
    .unwrap();
    catalog.apply_pack([
        (
            "ui/_global_variables.json",
            br##"{"$selected_marker":"|catalog|"}"##.as_slice(),
        ),
        (
            "ui/server_form.json",
            br##"{"namespace":"server_form",
                "content":{"size":["100%","100%"]},
                "content/server_form_factory":{"control_ids":{"long_form":"server_form.selected"}},
                "selected":{"type":"panel","controls":[{"choice":{
                    "type":"label","text":"Selected","size":[100,20],"visible":false,"enabled":false,
                    "bindings":[
                        {"binding_type":"view","source_property_name":"(not ((('prefix' + #title_text) - $selected_marker) = ('prefix' + #title_text)))","target_property_name":"#visible"},
                        {"binding_type":"view","source_property_name":"(not ((('prefix' + #title_text) - $selected_marker) = ('prefix' + #title_text)))","target_property_name":"#enabled"}
                    ]
                }}]}
            }"##.as_slice(),
        ),
    ]);
    let render = render_form(
        &FormModel::Action(ActionForm {
            title: "|catalog|".into(),
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
    .expect("form resolves");
    assert!(
        render
            .nodes
            .iter()
            .any(|node| matches!(&node.draw, Draw::Text { text, .. } if text == "Selected"))
    );
}
