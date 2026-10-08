use std::sync::Arc;

use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Draw, DrawNode, FormRender, RectOut, ResolvedControl};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiVisual};

use super::super::{EngineInputs, EngineOutput, FormEngine, ScreenArt};
use super::*;
use crate::ui_runtime::presentation::{TextMetrics, tests::fixture_font};

#[test]
fn native_font_height_uses_minimum_only_for_first_line_and_pitch_for_newlines() {
    let default = WIDTH_EXTRA;
    assert_eq!(
        text_height(default, TEXT_PITCH as f32, 1),
        TEXT_PITCH as f32
    );
    assert_eq!(
        text_height(default, TEXT_PITCH as f32, 3),
        (TEXT_PITCH * 3) as f32
    );
    let smaller = 0.5;
    let wrap = TEXT_PITCH as f32 * smaller;
    let minimum = ((smaller - default) * 0.5 + default) * BOX_EXTRA;
    assert!(minimum > wrap * smaller);
    assert_eq!(text_height(smaller, wrap, 1), minimum);
    assert_eq!(text_height(smaller, wrap, 2), minimum + wrap * smaller);
    let larger = 2.0;
    let wrap = TEXT_PITCH as f32 * larger;
    assert_eq!(text_height(larger, wrap, 2), wrap * larger * 2.0);
}

#[test]
fn native_mouse_offset_and_integer_box_extents() {
    let anchor = [30.0, 50.0];
    let text = [24.75, TEXT_PITCH as f32];
    let rect = box_rect(anchor, text, [300.0, 200.0]);
    assert_eq!(rect.x, f64::from(anchor[0] + MOUSE_OFFSET[0]));
    assert_eq!(rect.y, f64::from(anchor[1] + MOUSE_OFFSET[1]));
    assert_eq!(rect.w, f64::from(text[0].ceil() + WIDTH_EXTRA + BOX_EXTRA));
    assert_eq!(rect.h, f64::from(text[1] + BOX_EXTRA));
}

#[test]
fn right_overflow_flips_left_of_pointer_not_to_screen_edge() {
    let anchor = [195.0, 50.0];
    let rect = box_rect(anchor, [24.0, TEXT_PITCH as f32], [200.0, 200.0]);
    assert_eq!(rect.x + rect.w, f64::from(anchor[0] - MOUSE_OFFSET[0]));
    assert_eq!(rect.y, f64::from(anchor[1] + MOUSE_OFFSET[1]));
}

#[test]
fn bottom_overflow_moves_box_up_without_changing_horizontal_offset() {
    let viewport = [300.0, 200.0];
    let anchor = [30.0, viewport[1]];
    let rect = box_rect(anchor, [24.0, TEXT_PITCH as f32], viewport);
    assert_eq!(rect.x, f64::from(anchor[0] + MOUSE_OFFSET[0]));
    assert_eq!(rect.y + rect.h, f64::from(viewport[1]));
}

#[test]
fn oversized_box_centers_above_pointer_and_native_does_not_top_clamp() {
    let anchor = [100.0, 4.0];
    let rect = box_rect(anchor, [220.0, TEXT_PITCH as f32], [200.0, 200.0]);
    assert_eq!(rect.x + rect.w * 0.5, f64::from(anchor[0]));
    assert_eq!(rect.y + rect.h, f64::from(anchor[1]));
    assert!(rect.y < 0.0);
    let short = box_rect(anchor, [24.0, TEXT_PITCH as f32], [300.0, 200.0]);
    assert_eq!(short.y, f64::from(anchor[1] + MOUSE_OFFSET[1]));
    assert!(short.y < 0.0);
}

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
    let mut engine = FormEngine::new(assets, Catalog::default(), 2);
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
    engine.set_server_atlas(
        super::super::super::server_pack::ServerAtlas::new(&files, None, 1),
        3,
    );
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
            EngineInputs {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1280.0, 720.0],
                translate: &|_| None,
                language: [0; 3],
            },
            EngineOutput {
                nodes: &mut nodes,
                next: &mut next,
                overlay: &[],
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
