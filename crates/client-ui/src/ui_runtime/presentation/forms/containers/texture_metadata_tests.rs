use super::*;
use std::cell::Cell;

struct Text;
impl json_ui::TextMeasure for Text {
    /// Measures fixture text without consulting platform fonts.
    fn extent(&self, _: &str) -> [f64; 2] {
        [10.0; 2]
    }
}
struct Texture(Cell<[f64; 2]>);
impl json_ui::TextureSource for Texture {
    /// Publishes caller-selected dimensions for the retained measurement fixture.
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        Some(json_ui::TextureMeta::plain(self.0.get()))
    }
}

#[test]
fn texture_refresh_remeasures_inventory_and_preserves_once_bindings() {
    let mut catalog = json_ui::Catalog::default();
    catalog.apply_pack([
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/chest.json"]}"#.as_slice()),
        ("ui/chest.json", br##"{"namespace":"chest","small_chest_screen":{"type":"screen","controls":[
            {"sheet":{"type":"image","texture":"t","size":["default",10],"default_size_scales_to_ratio":true}},
            {"label":{"type":"label","size":[50,10],"text":"#text","bindings":[{"binding_name":"#value","binding_name_override":"#text","binding_condition":"once"}]}}
        ]}}"##.as_slice()),
    ]);
    let catalog = Arc::new(catalog);
    let (context, view) = (json_ui::Context::desktop(), ViewState::default());
    let texture = Texture(Cell::new([20.0, 10.0]));
    let env = json_ui::LayoutEnv {
        text: &Text,
        textures: &texture,
    };
    let mut data = DataSource::new();
    data.set_global("#value", Scalar::Text("first".into()));
    let mut cache = None;
    let render = |cache: &mut Option<ScreenCache>, data: &DataSource| {
        ScreenCache::render(
            cache,
            &catalog,
            "chest.small_chest_screen",
            &context,
            data,
            &view,
            [100.0; 2],
            &env,
            (1.0, [0; 3]),
        )
        .unwrap()
    };
    let old = render(&mut cache, &data);
    assert_eq!(
        old.nodes.iter().find(|n| n.name == "sheet").unwrap().dest.w,
        20.0
    );
    texture.0.set([60.0, 10.0]);
    cache.as_mut().unwrap().invalidate_textures();
    let new = render(&mut cache, &data);
    assert_eq!(
        new.nodes.iter().find(|n| n.name == "sheet").unwrap().dest.w,
        60.0
    );
    assert_eq!(
        old.nodes.iter().find(|n| n.name == "sheet").unwrap().dest.w,
        20.0
    );
    drop(old);
    drop(new);
    cache.as_mut().unwrap().invalidate_textures();
    data.set_global("#value", Scalar::Text("second".into()));
    let new = render(&mut cache, &data);
    assert!(
        new.nodes
            .iter()
            .any(|n| matches!(&n.draw, Draw::Text { text, .. } if text == "first")),
        "metadata invalidation preserves live once bindings"
    );
}
