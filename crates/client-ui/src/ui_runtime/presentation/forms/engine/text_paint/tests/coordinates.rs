//! Original fixture matching the observed coordinate control's geometry, through real label
//! measurement, JSON-UI layout, retained painting and rasterization; no game assets are needed.

use std::sync::Arc;

use assets::{
    FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, RuntimeUiAssets, UiAtlasPage,
    UiNineSlice, UiSidecar, UiTexturePlacement, encode_font_catalog, encode_ui_catalog,
};
use image::RgbaImage;
use json_ui::{Catalog, Context, DataSource, HUD_SCREEN, render_screen};
use sha2::{Digest, Sha256};
use ui::{DpiScale, SafeArea, UiScale, UiTree};

use super::super::super::{EngineInputs, EngineOutput, FormEngine, ScreenArt};
use crate::ui_runtime::presentation::forms::{pages, snapshot::rasterize};
use crate::ui_runtime::presentation::{TextMetrics, UiPresentationRuntime, rect};
use crate::ui_runtime::render_adapter::{UiRenderViewport, adapt_ui_draw_list};

const BACKGROUND: [u8; 4] = [22, 33, 44, 255];

/// Eight-pixel-tall rectangles with a 48-pixel total advance and 47-pixel ink span. Their
/// geometry isolates the label box and origin from the accepted open font's actual glyph art.
fn font() -> Arc<RuntimeFontCatalog> {
    let mut pixels = vec![0; 128 * 16 * 4];
    let mut glyphs = Vec::new();
    let mut x = 0u16;
    for (codepoint, advance) in [
        (' ', 4),
        (',', 2),
        ('-', 6),
        ('1', 6),
        ('2', 6),
        ('4', 6),
        ('7', 6),
        ('\u{fffd}', 6),
    ] {
        let width = (advance - 1) * ui::FONT_DESIGN_PIXEL_TEXELS as u16;
        if codepoint != ' ' {
            for row in 0..16usize {
                for column in x..x + width {
                    let at = (row * 128 + usize::from(column)) * 4;
                    pixels[at..at + 4].fill(255);
                }
            }
        }
        glyphs.push(GlyphMetrics {
            codepoint,
            page: 0,
            uv: [x, 0, x + width, 16],
            bearing: [0, -14],
            advance_64: (advance * ui::FONT_DESIGN_PIXEL_TEXELS as u16 * 64) as i16,
        });
        x += width + 2;
    }
    let pixels = pixels.into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/coordinate-rectangles.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [3; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 128,
        height: 16,
        pixels: FontPixels::Rgba8(pixels),
    };
    let manifest = [4; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}

/// A two-pixel stack spacer and content-sized background with the observed negative label
/// offset. Required pack indices explicitly admit the original fixture definition.
fn catalog(scale: f32, font_type: &str) -> Catalog {
    let mut json = serde_json::json!({
        "namespace": "hud",
        "hud_screen": {
            "type": "stack_panel", "orientation": "vertical", "size": ["100%", "100%"],
            "anchor_from": "top_left", "anchor_to": "top_left",
            "controls": [
                {"padding": {"type": "panel", "size": [0, 2]}},
                {"coordinates": {
                    "type": "image", "texture": "textures/ui/coordinate-fixture",
                    "size": ["100%c + 6px", "100%c + 2px"],
                    "anchor_from": "top_left", "anchor_to": "top_left",
                    "controls": [{"coordinate_text": {
                        "type": "label", "text": "-17, 47, 2", "localize": false,
                        "anchor_from": "bottom_middle", "anchor_to": "bottom_middle",
                        "offset": [0, -1], "shadow": true,
                        "font_scale_factor": scale, "font_type": font_type
                    }}]
                }}
            ]
        }
    });
    if scale == 0.5 {
        // A fixed authored box isolates the paint inset from smaller fonts' minimum heights.
        json["hud_screen"]["controls"][1]["coordinates"]["controls"][0]["coordinate_text"]["size"] =
            serde_json::json!([24, 10]);
    }
    let json = json.to_string();
    Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/coordinate-fixture.json"]}"#.as_slice(),
        ),
        ("ui/coordinate-fixture.json", json.as_bytes()),
    ])
    .unwrap()
}

/// Paint the authored child-content padding and negative child offset using the same adapter
/// that paints the live HUD. Returns half-open physical background/ink bounds and text origin.
fn paint(physical: [u32; 2], scale: f32, named: bool) -> ([u32; 4], [u32; 4], f32) {
    let base_font = font();
    let font = if named {
        Arc::new(
            base_font
                .with_named_font("alternative", &base_font)
                .unwrap(),
        )
    } else {
        base_font
    };
    let mut presentation = UiPresentationRuntime::new(Arc::clone(&font)).unwrap();
    let page = UiAtlasPage {
        width: 6,
        height: 6,
        rgba8: BACKGROUND.repeat(36).into(),
    };
    let placement = UiTexturePlacement {
        path: "textures/ui/coordinate-fixture".into(),
        page: 0,
        x: 0,
        y: 0,
        width: 6,
        height: 6,
    };
    let sidecar = (
        "textures/ui/coordinate-fixture".into(),
        UiSidecar {
            base_size: [6.0, 6.0],
            nineslice: Some(UiNineSlice {
                left: 2.0,
                top: 2.0,
                right: 2.0,
                bottom: 2.0,
            }),
        },
    );
    let bytes = encode_ui_catalog([2; 32], &[page], &[placement], &[sidecar], &[]).unwrap();
    let assets = Arc::new(RuntimeUiAssets::decode(&bytes).unwrap());
    let (textures, first_page) = pages::with_ui_pages(&presentation.textures, &assets).unwrap();
    let catalog = catalog(scale, if named { "alternative" } else { "default" });
    let engine = FormEngine::new(assets, Catalog::default(), first_page);
    let dpi = DpiScale::new(1.0).unwrap();
    let metrics = TextMetrics::for_viewport(physical, dpi, None);
    let mut nodes = Vec::new();
    let mut next = 1;
    engine
        .draw(
            ScreenArt::default(),
            EngineInputs {
                layouts: &mut presentation.layouts,
                font: &font,
                metrics,
                solid_page: presentation.solid_texture_page,
                safe_area: SafeArea::ZERO,
                content: physical.map(|side| side as f32),
                translate: &|_| None,
                language: [0; 3],
            },
            EngineOutput {
                nodes: &mut nodes,
                next: &mut next,
                overlay: &[],
            },
            |env, root| {
                render_screen(
                    HUD_SCREEN,
                    &catalog,
                    &Context::desktop(),
                    &DataSource::new(),
                    root,
                    env,
                    &Default::default(),
                )
            },
        )
        .unwrap()
        .expect("coordinate fixture draws");
    let text_top = nodes
        .iter()
        .find_map(|node| {
            matches!(node.visual(), ui::UiVisual::Text { .. }).then_some(node.bounds().min().y())
        })
        .expect("coordinate label draws");
    let mut tree = UiTree::new(nodes).unwrap();
    tree.layout(
        rect(0.0, 0.0, physical[0] as f32, physical[1] as f32).unwrap(),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();
    let input = adapt_ui_draw_list(
        &tree.build_draw_list().unwrap(),
        Arc::new(textures),
        UiRenderViewport {
            physical_size: physical,
            dpi_scale: dpi,
            safe_area: SafeArea::ZERO,
        },
    )
    .unwrap();
    let image = rasterize(&input);
    (
        bounds(&image, |pixel| pixel == BACKGROUND),
        bounds(&image, |pixel| pixel == [255; 4]),
        text_top,
    )
}

/// Half-open physical bounds of a fixture's nonempty pixel class.
fn bounds(image: &RgbaImage, matches: impl Fn([u8; 4]) -> bool) -> [u32; 4] {
    let mut bounds = [u32::MAX, u32::MAX, 0, 0];
    for (x, y, _) in image
        .enumerate_pixels()
        .filter(|(_, _, pixel)| matches(pixel.0))
    {
        bounds = [
            bounds[0].min(x),
            bounds[1].min(y),
            bounds[2].max(x + 1),
            bounds[3].max(y + 1),
        ];
    }
    assert_ne!(bounds[0], u32::MAX, "fixture pixels are drawn");
    bounds
}

#[test]
fn coordinate_label_matches_native_content_height_and_paint_top() {
    for (physical, gui_scale) in [([854, 459], 1), ([1280, 720], 2)] {
        let (background, ink, _) = paint(physical, 1.0, false);
        assert_eq!(background, [0, 2, 54, 14].map(|edge| edge * gui_scale));
        assert_eq!(ink, [3, 4, 50, 12].map(|edge| edge * gui_scale));
    }
}

#[test]
fn default_label_top_padding_does_not_scale_with_font_size() {
    let (background, _, top) = paint([854, 459], 0.5, false);
    assert_eq!(background, [0, 2, 30, 14]);
    assert_eq!(top, 4.0, "native UI top padding stays one GUI pixel");
}

#[test]
fn bitmap_label_correction_preserves_attached_named_font_metrics() {
    let (background, ink, top) = paint([854, 459], 1.0, true);
    assert_eq!(background, [0, 2, 54, 13]);
    assert_eq!(ink, [3, 3, 50, 11]);
    assert_eq!(top, 3.0);
}
