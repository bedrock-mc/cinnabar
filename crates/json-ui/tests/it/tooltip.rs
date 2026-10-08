//! Hover text receives its authored maximum width, not just # bindings.
use std::collections::BTreeMap;

use json_ui::{
    Draw, LayoutEnv, ResolvedControl, TextMeasure, TextureMeta, TextureSource, emit, layout,
};
use serde_json::json;

struct NoContent;
impl TextMeasure for NoContent {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0; 2]
    }
}
impl TextureSource for NoContent {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

#[test]
fn custom_hover_renderer_receives_native_width_property() {
    let properties: BTreeMap<_, _> = [
        ("renderer".to_owned(), json!("hover_text_renderer")),
        ("hover_text_max_width".to_owned(), json!(60)),
        ("#hover_text".to_owned(), json!("Long tooltip text")),
        ("size".to_owned(), json!([16, 16])),
    ]
    .into();
    let control = ResolvedControl {
        name: "hover".into(),
        control_type: Some("custom".into()),
        base: None,
        unresolved_base: None,
        properties: properties.into(),
        children: vec![],
        factory: None,
    };
    let env = LayoutEnv {
        text: &NoContent,
        textures: &NoContent,
    };
    let output = emit(&layout(&control, [200.0, 200.0], &env), &env);
    let Draw::Custom { data, .. } = &output[0].draw else {
        panic!("custom renderer");
    };
    assert_eq!(
        data.get("hover_text_max_width"),
        control.properties.get("hover_text_max_width")
    );
    assert_eq!(
        data.get("#hover_text"),
        control.properties.get("#hover_text")
    );
}
