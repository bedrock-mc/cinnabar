use std::{collections::BTreeMap, sync::Arc};

use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Draw, DrawNode, FormRender, RectOut, ResolvedControl};
use ui::{DpiScale, SafeArea, TextLayoutCache, TextShadow, UiNode, UiVisual};

use serde_json::Value;
use ui::FONT_DESIGN_PIXEL_TEXELS;
use ui_engine::{
    atlas::ServerAtlas,
    engine::{
        FormEngine, ScreenArt,
        screen::PaintFrame,
        tooltip::{BACKGROUND_TEXTURE, MOUSE_OFFSET, RENDERER, TEXT_OFFSET, TEXT_PITCH},
    },
    text_metrics::TextMetrics,
};

use super::super::screen_registry::registry;
use ui_engine::test_support::fixture_font;

/// Synthetic art, not a redistributed vanilla bitmap. The actual runtime pack
/// determines the border texels and sidecar; this fixture verifies their path.
fn engine() -> FormEngine {
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
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([22, 33, 44, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let files = [
        (format!("{BACKGROUND_TEXTURE}.png"), png),
        (
            format!("{BACKGROUND_TEXTURE}.json"),
            br#"{"nineslice_size":4,"base_size":[16,16]}"#.to_vec(),
        ),
    ];
    engine.set_server_atlas(ServerAtlas::new(&files, None, 1), 3);
    engine
}

fn draw(engine: &FormEngine, text: &str, max_width: impl Into<Value>) -> (Vec<UiNode>, f32) {
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), None);
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let control = ResolvedControl {
        name: "hover".to_owned(),
        control_type: Some("custom".to_owned()),
        base: None,
        unresolved_base: None,
        properties: BTreeMap::new().into(),
        children: vec![],
        factory: None,
    };
    let dest = RectOut {
        x: 0.0,
        y: 0.0,
        w: 16.0,
        h: 16.0,
    };
    let render = FormRender {
        bound: control,
        nodes: vec![DrawNode {
            name: "hover".into(),
            key: "hover".into(),
            dest,
            clip: dest,
            layer: 5,
            alpha: 0.5,
            anim: None,
            draw: Draw::Custom {
                renderer: RENDERER.into(),
                data: [
                    ("#hover_text".to_owned(), Value::from(text)),
                    ("hover_text_max_width".to_owned(), max_width.into()),
                ]
                .into(),
            },
            gates: vec![],
        }],
        hits: vec![].into(),
        report: Default::default(),
        cancel_target: None,
        root_panel: None,
    };
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let mut nodes = Vec::new();
    let mut next = 1;
    engine
        .draw(
            ScreenArt {
                pointer: Some([80.0, 90.0]),
                ..Default::default()
            },
            &mut PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1280.0, 720.0],
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            },
            |_, _| Some(render),
        )
        .unwrap();
    (nodes, px)
}

#[test]
fn custom_renderer_resides_native_background_and_emits_nine_slices_then_unshadowed_text() {
    let engine = engine();
    let (nodes, px) = draw(&engine, "Shield", 0);
    let sprites: Vec<_> = nodes
        .iter()
        .filter(|node| matches!(node.visual(), UiVisual::Sprite { .. }))
        .collect();
    assert_eq!(sprites.len(), 9);
    assert!(
        nodes
            .iter()
            .all(|node| !matches!(node.visual(), UiVisual::Solid { .. }))
    );
    let text = nodes.last().unwrap();
    let UiVisual::Text {
        color,
        shadow,
        layout,
    } = text.visual()
    else {
        panic!("text paints last");
    };
    assert_eq!(*color, [255, 255, 255, 128]);
    assert_eq!(*shadow, TextShadow::None);
    for sprite in sprites {
        assert!(matches!(
            sprite.visual(),
            UiVisual::Sprite {
                texture_page: 3,
                color: [255, 255, 255, 128],
                ..
            }
        ));
    }
    assert_eq!(
        text.bounds().min().x(),
        (80.0 + MOUSE_OFFSET[0] + TEXT_OFFSET) * px
    );
    assert_eq!(
        text.bounds().min().y(),
        (90.0 + MOUSE_OFFSET[1] + TEXT_OFFSET) * px
    );
    assert_eq!(layout.size_64()[1] as f32 / 64.0 / px, TEXT_PITCH as f32);
    let parent = nodes
        .iter()
        .find(|node| Some(node.id()) == text.parent())
        .unwrap();
    assert_eq!(parent.bounds().width(), 1280.0);
    assert_eq!(parent.bounds().height(), 720.0);
}

#[test]
fn tooltip_wraps_only_when_the_pack_requests_a_positive_maximum() {
    let engine = engine();
    let layout = |nodes: Vec<UiNode>| {
        nodes
            .iter()
            .find_map(|node| match node.visual() {
                UiVisual::Text { layout, .. } => Some(Arc::clone(layout)),
                _ => None,
            })
            .unwrap()
    };
    let unrestricted = layout(draw(&engine, "Shield Shield Shield", 0).0);
    let restricted = layout(draw(&engine, "Shield Shield Shield", 40).0);
    assert_eq!(unrestricted.line_count(), 1);
    assert!(restricted.line_count() > 1);
    let fractional = layout(draw(&engine, "Shield Shield Shield", 40.5).0);
    assert_eq!(
        fractional.line_count(),
        1,
        "native accepts only integer JSON widths"
    );
    let empty_lines = layout(draw(&engine, "Shield\n\nShield", 0).0);
    assert_eq!(empty_lines.line_count(), 3, "native counts every newline");
}
