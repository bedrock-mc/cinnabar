use super::*;

use crate::{
    atlas::ServerAtlas,
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintFrame, ScreenRegistry},
    },
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
    text_metrics::TextMetrics,
};
use assets::{RuntimeUiAssets, encode_ui_catalog};
use json_ui::{Catalog, Context, DataSource};
use std::sync::Arc;
use ui::{DpiScale, SafeArea, TextLayoutCache, UiVisual};

struct Offline;
impl RemoteSource for Offline {
    /// Tooltip fixtures never request downloads.
    fn state(&self, _: &str) -> RemoteImage {
        RemoteImage::Failed
    }
}

#[test]
fn zero_sized_hover_control_paints_a_tooltip_at_the_pointer() {
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let registry = REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop()));
    let bytes = encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    let mut engine = FormEngine::new(
        Arc::new(RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        registry,
        Arc::new(Offline),
        ImageWork::inline(),
    );
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 10, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    engine.set_server_atlas(
        ServerAtlas::new(
            &[
                (format!("{BACKGROUND_TEXTURE}.png"), png),
                (
                    format!("{BACKGROUND_TEXTURE}.json"),
                    br#"{"nineslice_size":4,"base_size":[16,16]}"#.to_vec(),
                ),
            ],
            None,
            1,
        )
        .with_image_work(ImageWork::inline()),
        1,
    );
    let mut catalog = Catalog::default();
    catalog.overlay_text(
        "ui/fixture.json",
        &serde_json::json!({"namespace":"fixture","tooltip":{
        "type":"custom","renderer":RENDERER,"size":[0,0],"allow_clipping":false,
        "property_bag":{"#hover_text":"Details"}}})
        .to_string(),
    );
    let root = json_ui::resolve(&catalog, "fixture.tooltip", &Context::desktop())
        .control
        .unwrap();
    let root = json_ui::bind(&root, &DataSource::new(), &json_ui::EmptyLibrary);
    let font = fixture_font();
    let mut nodes = Vec::new();
    let mut next = 1;
    engine
        .draw(
            ScreenArt {
                pointer: Some([40.0, 50.0]),
                ..Default::default()
            },
            &mut PaintFrame {
                layouts: &mut TextLayoutCache::new(4, 1 << 20),
                font: &font,
                metrics: TextMetrics::for_viewport(
                    [1280, 720],
                    DpiScale::new(1.0).unwrap(),
                    Some(2),
                ),
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
            |env, size| Some(json_ui::render_bound(root, size, env, &Default::default())),
        )
        .unwrap();
    assert!(
        nodes
            .iter()
            .any(|node| matches!(node.visual(), UiVisual::Text { .. })),
        "zero-sized hover controls still paint text"
    );
    assert_eq!(
        nodes
            .iter()
            .filter(|node| matches!(node.visual(), UiVisual::Sprite { .. }))
            .count(),
        9
    );
}

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
