use std::sync::Arc;

use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Context, DataSource, ViewState};
use ui::{DpiScale, FONT_DESIGN_PIXEL_TEXELS, SafeArea, TextLayoutCache, UiVisual};

use super::super::screen_registry::registry;
use ui_engine::atlas::ServerAtlas;
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{FormEngine, ScreenArt, screen::PaintFrame};
use ui_engine::frame::EngineFrame;
use ui_engine::test_support::fixture_font;
use ui_engine::text_metrics::TextMetrics;

/// Paints an authored banner control against independent pack artwork.
fn render() -> (Vec<ui::UiNode>, EngineFrame) {
    let page = UiAtlasPage {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    };
    let bytes = encode_ui_catalog([1; 32], &[page], &[], &[], &[]).unwrap();
    let assets = Arc::new(RuntimeUiAssets::decode(&bytes).unwrap());
    let mut engine = FormEngine::new(
        assets,
        Catalog::default(),
        2,
        registry(),
        crate::ui_runtime::presentation::forms::texture_sources::downloads(),
        crate::ui_runtime::presentation::forms::texture_sources::image_work(),
    );
    let files = ["base", "stripe_right", "border"].map(|name| {
        let mut pixels = image::RgbaImage::new(64, 64);
        for y in 1..41 {
            for x in 1..21 {
                if name == "base"
                    || (name == "stripe_right" && x >= 11)
                    || (name == "border" && (x <= 3 || x >= 18 || y <= 3 || y >= 38))
                {
                    pixels.put_pixel(x, y, image::Rgba([255; 4]));
                }
            }
        }
        let mut png = Vec::new();
        pixels
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        (format!("textures/entity/banner/banner_{name}.png"), png)
    });
    engine.set_server_atlas(ServerAtlas::new(&files, None, 1), 3);
    let screen = serde_json::json!({
        "namespace": "loom", "loom_screen": {"type": "panel", "controls": [
            {"banner": {"type": "custom", "renderer": "banner_pattern_renderer", "size": [100, 200],
                "anchor_from": "top_left", "anchor_to": "top_left", "offset": [110, 20],
                "property_bag": {"#banner_patterns": "0_34_1", "#banner_colors": "15_1_4"}}},
            {"unknown": {"type": "custom", "renderer": "camera_renderer", "size": [20, 40]}}
        ]}
    })
    .to_string();
    let catalog = Arc::new(
        Catalog::from_files([
            ("ui/_global_variables.json", &b"{}"[..]),
            (
                "ui/_ui_defs.json",
                &br#"{"ui_defs":["ui/loom_test.json"]}"#[..],
            ),
            ("ui/loom_test.json", screen.as_bytes()),
        ])
        .unwrap(),
    );
    let physical = [320, 240];
    let metrics = TextMetrics::for_viewport(physical, DpiScale::new(1.0).unwrap(), None);
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(256, 1 << 20);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let mut cached = CachedScreen::default();
    let view = ViewState::default();
    let frame = engine
        .draw(
            ScreenArt::default(),
            &mut PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: physical.map(|side| side as f32),
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            },
            |env, root| {
                cached.render_with(
                    "loom.loom_screen",
                    &catalog,
                    &Context::default(),
                    DataSource::new(),
                    (root, px, [0; 3]),
                    env,
                    &view,
                )
            },
        )
        .unwrap()
        .expect("the screen lays out");
    (nodes, frame)
}

/// Layers keep binding order and dye.
#[test]
fn banner_custom_control_paints_ordered_front_layers() {
    let (nodes, _frame) = render();
    let colors: Vec<_> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Sprite { color, uv, .. } => {
                assert_eq!(uv[2] - uv[0], 20);
                assert_eq!(uv[3] - uv[1], 40);
                Some(*color)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        colors,
        [[240, 240, 240, 255], [176, 46, 38, 255], [60, 68, 170, 255]]
    );
}
