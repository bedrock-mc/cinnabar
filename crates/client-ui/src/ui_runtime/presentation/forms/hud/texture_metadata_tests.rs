use super::*;
use json_ui::{Context, DataSource, ViewState};
use ui_engine::{
    atlas::ServerAtlas,
    cached_screen::CachedScreen,
    textures::{TextureSet, Textures},
};

/// Creates a source with known dimensions without asynchronous metadata updates.
fn atlas(size: [u32; 2]) -> ServerAtlas {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::new(size[0], size[1])
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    ServerAtlas::new(
        &[("textures/ui/measure.png".into(), bytes.into_inner())],
        None,
        1,
    )
}

struct Text;
impl json_ui::TextMeasure for Text {
    /// Measures fixture text without platform fonts.
    fn extent(&self, _: &str) -> [f64; 2] {
        [10.0; 2]
    }
}

#[test]
fn texture_only_reload_remeasures_shared_screens_without_resetting_bindings() {
    let mut catalog = Catalog::default();
    catalog.apply_pack([
        ("ui/_ui_defs.json", br#"{"ui_defs":["ui/measure.json"]}"#.as_slice()),
        ("ui/measure.json", br##"{"namespace":"measure","root":{"type":"screen","controls":[
            {"image":{"type":"image","texture":"textures/ui/measure","size":["default",10],"default_size_scales_to_ratio":true}},
            {"label":{"type":"label","size":[50,10],"text":"#text","bindings":[{"binding_name":"#value","binding_name_override":"#text","binding_condition":"once"}]}}
        ]}}"##.as_slice()),
    ]);
    let catalog = Arc::new(catalog);
    let assets = crate::test_support::mini_carrier();
    let mut set = TextureSet::new(
        0,
        crate::ui_runtime::presentation::forms::texture_sources::downloads(),
        crate::ui_runtime::presentation::forms::texture_sources::image_work(),
    );
    let mut screen = CachedScreen::default();
    let (context, view) = (Context::desktop(), ViewState::default());
    let mut data = DataSource::new();
    data.set_global("#value", json_ui::Scalar::Text("first".into()));
    let mut data = Arc::new(data);
    let mut last_revision = set.metadata_revision();
    for width in [20, 60] {
        set.set_atlas(atlas([width, 10]), 1);
        assert_ne!(set.metadata_revision(), last_revision);
        last_revision = set.metadata_revision();
        let atlas = set.lock();
        assert_eq!(
            atlas.metadata_revision(),
            0,
            "replacement freshness needs the source namespace too"
        );
        let textures = Textures {
            assets: &assets,
            set: &set,
            atlas: &atlas,
            images: None,
        };
        let env = json_ui::LayoutEnv {
            text: &Text,
            textures: &textures,
        };
        let render = screen
            .render_shared_with(
                "measure.root",
                &catalog,
                &context,
                Arc::clone(&data),
                ([100.0; 2], 1.0, [0; 3]),
                &env,
                &view,
            )
            .unwrap();
        assert_eq!(
            render
                .nodes
                .iter()
                .find(|n| n.name == "image")
                .unwrap()
                .dest
                .w,
            f64::from(width)
        );
        assert!(
            render
                .nodes
                .iter()
                .any(|n| matches!(&n.draw, json_ui::Draw::Text { text, .. } if text == "first"))
        );
        if width == 60 {
            Arc::make_mut(&mut data).set_global("#value", json_ui::Scalar::Text("second".into()));
            let render = screen
                .render_shared_with(
                    "measure.root",
                    &catalog,
                    &context,
                    Arc::clone(&data),
                    ([100.0; 2], 1.0, [0; 3]),
                    &env,
                    &view,
                )
                .unwrap();
            assert!(
                render.nodes.iter().any(
                    |n| matches!(&n.draw, json_ui::Draw::Text { text, .. } if text == "first")
                ),
                "texture-only reload preserves once bindings across subsequent data refresh"
            );
        }
        let (_, allocations) = test_alloc::count(|| {
            screen
                .render_shared_with(
                    "measure.root",
                    &catalog,
                    &context,
                    Arc::clone(&data),
                    ([100.0; 2], 1.0, [0; 3]),
                    &env,
                    &view,
                )
                .unwrap();
        });
        assert_eq!(
            allocations, 0,
            "unchanged shared screens reuse their metadata-dependent layout"
        );
    }
}
