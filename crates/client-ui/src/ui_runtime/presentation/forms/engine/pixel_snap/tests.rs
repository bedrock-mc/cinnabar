use std::sync::Arc;

use assets::{RuntimeUiAssets, UiAtlasPage, encode_ui_catalog};
use json_ui::{Catalog, Context, DataSource, ViewState};
use ui::{DpiScale, SafeArea, TextLayoutCache, UiNode, UiVisual};

use super::super::super::hud::CachedScreen;
use super::super::super::server_pack::ServerAtlas;
use super::super::{EngineInputs, EngineOutput, FormEngine, ScreenArt};
use super::{positioned, snapped};
use crate::ui_runtime::presentation::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics, tests::fixture_font};

const PAINT_VIEWPORT: [u32; 2] = [1440, 813];

/// Whether every edge of `rect` lies on a whole physical pixel at `dpi` physical pixels per
/// logical pixel.
fn whole(rect: [f32; 4], dpi: f32) -> bool {
    rect.iter()
        .all(|edge| ((edge * dpi) - (edge * dpi).round()).abs() < 1e-3)
}

/// A 195×152 terminal centred in a 480×271 GUI-unit screen sits at (142.5, 59.5); at GUI
/// scale 3 that is (427.5, 178.5) physical pixels, which vanilla truncates to (427, 178).
#[test]
fn a_control_centred_in_odd_free_space_lands_on_whole_pixels() {
    let rect = snapped([142.5, 59.5, 195.0, 152.0], 3.0, 3.0);
    assert_eq!(rect, [427.0, 178.0, 1012.0, 634.0]);
    assert!(whole(rect, 1.0));
}

/// Sizes round up to whole pixels from the truncated position, as vanilla adds
/// `ceil(w * s)` to `(int)(x * s)`.
#[test]
fn sizes_round_up_to_whole_pixels() {
    assert_eq!(
        snapped([0.5, 0.0, 0.5, 1.0], 3.0, 3.0),
        [1.0, 0.0, 3.0, 3.0]
    );
    assert_eq!(snapped([0.0, 0.0, 1.0 / 3.0, 1.0], 2.0, 2.0)[2], 1.0);
}

/// Positions truncate toward zero, as the `(int)` cast does, so a control left of the
/// origin moves right.
#[test]
fn negative_positions_truncate_toward_zero() {
    assert_eq!(
        snapped([-1.5, 0.0, 2.0, 2.0], 3.0, 3.0),
        [-4.0, 0.0, 2.0, 6.0]
    );
}

/// The snap is in physical pixels: at DPI 1.25 and GUI scale 3 (2.4 logical pixels per
/// unit) the logical edges are fractional and the physical ones whole.
#[test]
fn edges_are_whole_in_physical_pixels_at_any_dpi() {
    for (x, y) in [(142.5, 59.5), (7.0, 17.0), (0.25, 100.75), (333.3, 12.1)] {
        let rect = snapped([x, y, 18.0, 20.0], 3.0, 2.4);
        assert!(whole(rect, 1.25), "{x},{y}: {rect:?}");
    }
}

/// A position already whole in physical pixels stays put despite float noise.
#[test]
fn whole_positions_survive_float_noise() {
    assert_eq!(snapped([0.999_999_9, 0.0, 1.0, 1.0], 3.0, 3.0)[0], 3.0);
    assert_eq!(snapped([0.0, 0.0, 1.000_000_1, 1.0], 3.0, 3.0)[2], 3.0);
}

/// Adjacent slices of one nine-slice never open a gap between them.
#[test]
fn adjacent_slices_stay_joined() {
    for x in [142.5, 142.25, 0.75, 7.0] {
        for split in [0.5, 1.0, 1.5, 4.25] {
            let left = snapped([x, 0.0, split, 1.0], 3.0, 3.0);
            let right = snapped([x + split, 0.0, 6.0, 1.0], 3.0, 3.0);
            assert!(left[2] >= right[0], "{x}+{split}: {left:?} {right:?}");
        }
    }
}

/// Text moves to the pixel grid but keeps its size, so its wrap width is unchanged.
#[test]
fn text_keeps_its_size() {
    assert_eq!(
        positioned([142.5, 59.5, 10.25, 9.0], 3.0, 3.0),
        [427.0, 178.0, 457.75, 205.0]
    );
}

/// Nearest sampling inside the snapped quad stays inside its uv rect with margin: an 18×20
/// toolbar button background (rows 128..148 of its sheet, an opaque grid row at 127) at GUI
/// scale 3, on the half unit the live client drew it at. Unsnapped, the first pixel centre
/// samples exactly the texel edge at 128, where interpolation error picks row 127.
#[test]
fn nearest_sampling_stays_inside_the_uv_rect() {
    let [_, y0, _, y1] = snapped([127.5, 62.5, 18.0, 20.0], 3.0, 3.0);
    let (v0, v1) = (128.0_f32, 148.0_f32);
    // Rasterization covers the pixel rows whose centres lie in [y0, y1).
    let first = (y0 - 0.5).ceil() as i32;
    let last = (y1 - 0.5).ceil() as i32 - 1;
    assert!(first <= last);
    for row in first..=last {
        let centre = row as f32 + 0.5;
        let v = v0 + (centre - y0) / (y1 - y0) * (v1 - v0);
        assert!((v0..v1).contains(&v.floor()), "row {row} samples {v}");
        assert!(
            (v - v.round()).abs() > 0.1,
            "row {row} samples {v}, on a texel edge"
        );
    }
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
    let mut engine = FormEngine::new(assets, Catalog::default(), 2);
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
            EngineInputs {
                layouts: &mut layouts,
                font: &font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: physical.map(|side| side as f32 / dpi),
                translate: &|_| None,
                language: [0; 3],
            },
            EngineOutput {
                nodes: &mut nodes,
                next: &mut next,
                overlay: &[],
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
