use std::sync::Arc;

use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Context, DataSource, ViewState};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiVisual};

use super::super::screen_registry::registry;
use ui_engine::atlas::ServerAtlas;
use ui_engine::cached_screen::CachedScreen;
use ui_engine::engine::{FormEngine, ScreenArt, screen::PaintFrame};
use ui_engine::text_metrics::TextMetrics;
use {ui::FONT_DESIGN_PIXEL_TEXELS, ui_engine::test_support::fixture_font};

const PAINT_VIEWPORT: [u32; 2] = [1440, 813];

/// Whether every edge of `rect` lies on a whole physical pixel at `dpi` physical pixels per
/// logical pixel.
fn whole(rect: [f32; 4], dpi: f32) -> bool {
    rect.iter()
        .all(|edge| ((edge * dpi) - (edge * dpi).round()).abs() < 1e-3)
}

/// The nodes painted for `control`, centred by JSON-UI's default anchors in a 1440×813 window
/// (GUI scale 3, a 480×271 unit screen, so odd free space on both axes) at `dpi`.
fn paint(control: &str, dpi: f32) -> Vec<UiNode> {
    paint_texture(control, dpi, [16, 16], None)
}

/// Paints an image with authored texture dimensions and optional slice metadata.
fn paint_texture(
    control: &str,
    dpi: f32,
    dimensions: [u32; 2],
    sidecar: Option<&str>,
) -> Vec<UiNode> {
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
    image::RgbaImage::from_pixel(dimensions[0], dimensions[1], image::Rgba([22, 33, 44, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut files = vec![("textures/snap/button.png".to_owned(), png)];
    if let Some(sidecar) = sidecar {
        files.push((
            "textures/snap/button.json".to_owned(),
            sidecar.as_bytes().to_vec(),
        ));
    }
    engine.set_server_atlas(ServerAtlas::new(&files, None, 1), 3);
    let screen = format!(
        r#"{{
            "namespace": "snap",
            "screen": {{ "type": "panel", "controls": [{{ "control@snap.control": {{}} }}] }},
            "control": {control}
        }}"#
    );
    let catalog = Arc::new(
        Catalog::from_files([
            ("ui/_global_variables.json", &b"{}"[..]),
            ("ui/_ui_defs.json", &br#"{"ui_defs":["ui/snap.json"]}"#[..]),
            ("ui/snap.json", screen.as_bytes()),
        ])
        .unwrap(),
    );
    let physical = PAINT_VIEWPORT;
    let metrics = TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), None);
    assert_eq!(metrics.gui_scale, 3.0);
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let font = fixture_font();
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let (mut nodes, mut next) = (Vec::new(), 1);
    let mut cached = CachedScreen::default();
    let view = ViewState::default();
    engine
        .draw(
            ScreenArt::default(),
            &mut PaintFrame {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: physical.map(|side| side as f32 / dpi),
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            },
            |env, root| {
                assert_eq!(root.map(f64::round), [480.0, 271.0]);
                cached.render_with(
                    "snap.screen",
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
    nodes
}

/// A one-texel sliced cap must sample its own column across both half-width pieces.
#[test]
fn thin_nine_slice_caps_do_not_sample_the_neighboring_atlas_column() {
    let nodes = paint_texture(
        r#"{"type":"image","texture":"textures/snap/button","size":[1,22]}"#,
        1.0,
        [1, 22],
        Some(r#"{"base_size":[1,22],"nineslice_size":[1,1,1,1]}"#),
    );
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(
        ui::UiRect::new(
            ui::UiPoint::new(0.0, 0.0).unwrap(),
            ui::UiPoint::new(PAINT_VIEWPORT[0] as f32, PAINT_VIEWPORT[1] as f32).unwrap(),
        )
        .unwrap(),
        ui::UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();
    let draw = tree.build_draw_list().unwrap();
    assert!(!draw.vertices.is_empty(), "the cap draws");
    let column = draw
        .vertices
        .iter()
        .map(|vertex| vertex.uv[0])
        .reduce(f32::min)
        .unwrap()
        .floor();
    for quad in draw.vertices.as_chunks::<4>().0 {
        let (left, right) = (quad[0].position[0], quad[1].position[0]);
        for x in (left.ceil() as u32)..(right.ceil() as u32) {
            let fraction = (x as f32 + 0.5 - left) / (right - left);
            if !(0.0..1.0).contains(&fraction) {
                continue;
            }
            let sampled = quad[0].uv[0] + (quad[1].uv[0] - quad[0].uv[0]) * fraction;
            assert_eq!(
                sampled.floor(),
                column,
                "cap pixel {x} samples foreign column {sampled}"
            );
        }
    }
}

/// A 195×152 image's absolute logical bounds (see [`paint`]).
fn centred_image(dpi: f32) -> [f32; 4] {
    let nodes = paint(
        r#"{
            "type": "image",
            "texture": "textures/snap/button",
            "size": [195, 152],
            "keep_ratio": false
        }"#,
        dpi,
    );
    let sprite = nodes
        .iter()
        .find(|node| matches!(node.visual(), UiVisual::Sprite { .. }))
        .expect("the image paints");
    absolute(&nodes, sprite)
}

/// `node`'s bounds plus its clip group's origin.
fn absolute(nodes: &[UiNode], node: &UiNode) -> [f32; 4] {
    let parent = nodes
        .iter()
        .find(|parent| Some(parent.id()) == node.parent())
        .expect("a clip group");
    let [x, y] = [parent.bounds().min().x(), parent.bounds().min().y()];
    let bounds = node.bounds();
    [
        x + bounds.min().x(),
        y + bounds.min().y(),
        x + bounds.max().x(),
        y + bounds.max().y(),
    ]
}

/// The painter places a centred image where vanilla does: (142.5, 59.5) units at GUI scale
/// 3 truncate to physical pixel (427, 178), and its size stays 585×456.
#[test]
fn the_painter_snaps_a_centred_image_to_whole_pixels() {
    assert_eq!(centred_image(1.0), [427.0, 178.0, 1012.0, 634.0]);
}

/// At DPI 1.25 the logical bounds are fractional but every physical edge is whole.
#[test]
fn the_painter_snaps_in_physical_pixels_under_dpi() {
    let rect = centred_image(1.25);
    assert!(whole(rect, 1.25), "{rect:?}");
    assert_eq!(
        rect.map(|edge| (edge * 1.25).round()),
        [427.0, 178.0, 1012.0, 634.0]
    );
}

/// Each line of a centred label starts on a whole physical pixel, as vanilla truncates each
/// line's alignment offset (`(int)(offset * guiScale) * invGuiScale` per line).
#[test]
fn the_painter_snaps_each_centred_line_to_whole_pixels() {
    for dpi in [1.0, 1.25] {
        let nodes = paint(
            r#"{
                "type": "label",
                "text": "a\nabc",
                "text_alignment": "center",
                "size": [101, 40]
            }"#,
            dpi,
        );
        let text = nodes
            .iter()
            .find(|node| matches!(node.visual(), UiVisual::Text { .. }))
            .expect("the label paints");
        let UiVisual::Text { layout, .. } = text.visual() else {
            unreachable!();
        };
        assert_eq!(layout.line_count(), 2);
        let [left, ..] = absolute(&nodes, text);
        for line in 0..2 {
            let start = layout
                .glyphs()
                .iter()
                .filter(|glyph| glyph.line == line)
                .map(|glyph| glyph.bounds_64[0])
                .min()
                .unwrap();
            let edge = (left + start as f32 / 64.0) * dpi;
            assert!(
                (edge - edge.round()).abs() < 0.02,
                "DPI {dpi} line {line} starts at physical {edge}"
            );
        }
    }
}
