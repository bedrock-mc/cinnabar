use super::*;

use assets::{
    FontPixels, FontRendering, FontTexturePage, GlyphMetrics, RuntimeUiAssets, encode_font_catalog,
    encode_ui_catalog,
};
use json_ui::{Catalog, Context, DataSource, bind_screen, render_bound, resolve};
use sha2::{Digest, Sha256};
use ui::{DpiScale, SafeArea, UiTree};

use crate::{
    caret::{TextSpot, nearest_boundary},
    engine::{
        FormEngine, ScreenArt,
        screen::{PaintFrame, ScreenRegistry},
    },
    frame::EngineFrame,
    sources::{ImageWork, RemoteImage, RemoteSource},
    test_support::fixture_font,
};

/// An em-tall rectangle isolates native face sizing from differences in glyph artwork.
fn native_font(em: u16) -> RuntimeFontCatalog {
    let pixels: Arc<[u8]> = vec![255; usize::from(em) * usize::from(em)].into();
    let page = FontTexturePage {
        source_path: "font/native-label-fixture.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: u32::from(em),
        height: u32::from(em),
        pixels: FontPixels::Coverage(pixels),
    };
    let face = assets::carriers::FONT_TEN.font_face.unwrap();
    let source = face.line_metrics_at(u32::from(em));
    let glyphs = ['0', '\u{fffd}'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [0, 0, em, em],
        bearing: [0, -(source.ascent_64 as i16 / 64)],
        advance_64: em as i16 * 64,
    });
    let manifest = [2; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    RuntimeFontCatalog::decode(&bytes, manifest)
        .unwrap()
        .with_rendering(FontRendering::NativeSdf)
        .with_line_metrics(source)
        .unwrap()
}

/// Measures the actual glyph height and distance between the two painted lines.
fn line_geometry(layout: &ui::TextLayout) -> [i32; 2] {
    assert_eq!(layout.line_count(), 2);
    assert!(!layout.ellipsized());
    let [first, second] = layout.glyphs() else {
        panic!("the label must draw one glyph on each line");
    };
    assert_eq!([first.line, second.line], [0, 1]);
    assert!(first.bounds_64[3] <= second.bounds_64[1], "lines overlap");
    [
        first.bounds_64[3] - first.bounds_64[1],
        second.bounds_64[1] - first.bounds_64[1],
    ]
}

#[test]
fn minecraftten_alias_normalizes_native_glyph_height_and_line_pitch() {
    let face = assets::carriers::FONT_TEN.font_face.unwrap();
    let alias = face.json_ui_alias.unwrap();
    let default = fixture_font();
    // Different raster sizes ensure sizing follows the catalog rather than a face-name table.
    for em in [26, 52] {
        let font = default
            .with_named_font(face.name, &native_font(em))
            .unwrap();
        assert_eq!(font.font_named(alias).rendering(), FontRendering::NativeSdf);
        for dpi in [1.0, 1.5] {
            for factor in [0.5, 1.0, 2.0, 13.0] {
                let metrics =
                    TextMetrics::for_viewport([1280, 720], DpiScale::new(dpi).unwrap(), Some(2));
                let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
                let options = TextOptions {
                    font_type: Some(alias.into()),
                    ..Default::default()
                };
                let dest = [0.0, 0.0, 800.0, 800.0];
                let mut layouts = TextLayoutCache::new(4, 1 << 20);
                let bitmap = layouts
                    .layout(painted_label_request(
                        metrics,
                        "0\n0",
                        dest,
                        &font,
                        factor,
                        &TextOptions::default(),
                        TextAlign::Left,
                        px,
                    ))
                    .unwrap();
                let native = layouts
                    .layout(painted_label_request(
                        metrics,
                        "0\n0",
                        dest,
                        &font,
                        factor,
                        &options,
                        TextAlign::Left,
                        px,
                    ))
                    .unwrap();
                let bitmap_geometry = line_geometry(&bitmap);
                let native_geometry = line_geometry(&native);
                if dpi == 1.0 && factor == 13.0 {
                    assert_eq!(bitmap_geometry, native_geometry);
                }
                for (bitmap, native) in bitmap_geometry.into_iter().zip(native_geometry) {
                    // Layout rounds scale to 1/1024 and glyph edges to 1/64 pixels.
                    assert!(
                        (bitmap - native).abs() <= 3,
                        "em {em}, DPI {dpi}, factor {factor}: {bitmap} != {native}"
                    );
                }
                let shape = LabelShape {
                    scale: f64::from(factor),
                    line_padding: 0.0,
                    hide_hyphen: false,
                };
                let measured = Measure {
                    layouts: &RefCell::new(&mut layouts),
                    font: &font,
                    metrics,
                    px,
                    translate: &|_| None,
                }
                .named_label("0\n0", alias, None, shape);
                assert_eq!(
                    measured,
                    native
                        .size_64()
                        .map(|size| f64::from(size) / 64.0 / f64::from(px))
                );
            }
        }
    }
}

#[test]
fn absent_alias_keeps_default_bitmap_label_metrics() {
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), Some(2));
    let options = TextOptions {
        font_type: assets::carriers::FONT_TEN
            .font_face
            .unwrap()
            .json_ui_alias
            .map(str::to_owned),
        ..Default::default()
    };
    let request = |options| {
        painted_label_request(
            metrics,
            "0\n0",
            [0.0, 0.0, 400.0, 400.0],
            &font,
            1.0,
            options,
            TextAlign::Left,
            metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32,
        )
    };
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let default_options = TextOptions::default();
    let bitmap = layouts.layout(request(&default_options)).unwrap();
    let fallback = layouts.layout(request(&options)).unwrap();
    assert_eq!(bitmap.key(), fallback.key());
    assert_eq!(bitmap.glyphs(), fallback.glyphs());
    assert_eq!(
        bitmap.key().line_height_64,
        TEXT_PITCH * FONT_DESIGN_PIXEL_TEXELS * 64
    );
    assert_eq!(bitmap.key().baseline_64, metrics.baseline_64);
}

#[test]
fn named_labels_paint_all_lines_at_their_measured_height() {
    let face = assets::carriers::FONT_TEN.font_face.unwrap();
    let alias = face.json_ui_alias.unwrap();
    let text = ["0"; 13].join("\n");
    for em in [26, 52] {
        let font = fixture_font()
            .with_named_font(face.name, &native_font(em))
            .unwrap();
        for dpi in [1.0, 1.5] {
            for factor in [0.5, 1.0, 2.0] {
                for padding in [0.0, 1.5] {
                    let metrics = TextMetrics::for_viewport(
                        [1280, 720],
                        DpiScale::new(dpi).unwrap(),
                        Some(2),
                    );
                    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
                    let mut layouts = TextLayoutCache::new(4, 1 << 20);
                    let measured = Measure {
                        layouts: &RefCell::new(&mut layouts),
                        font: &font,
                        metrics,
                        px,
                        translate: &|_| None,
                    }
                    .named_label(
                        &text,
                        alias,
                        None,
                        LabelShape {
                            scale: f64::from(factor),
                            line_padding: f64::from(padding),
                            hide_hyphen: false,
                        },
                    );
                    let options = TextOptions {
                        font_type: Some(alias.into()),
                        line_padding: padding,
                        ..Default::default()
                    };
                    let height = (measured[1] * f64::from(px)) as f32;
                    let request = painted_label_request(
                        metrics,
                        &text,
                        [0.0, 0.0, 800.0, height],
                        &font,
                        factor,
                        &options,
                        TextAlign::Left,
                        px,
                    );
                    let painted = layouts.layout(request).unwrap();
                    assert_eq!(
                        painted.line_count(),
                        13,
                        "em {em}, DPI {dpi}, factor {factor}, padding {padding}"
                    );
                    assert!(!painted.ellipsized());
                    assert_eq!(painted.size_64()[1] as f32 / 64.0, height);
                    let shortened = layouts
                        .layout(painted_label_request(
                            metrics,
                            &text,
                            [0.0, 0.0, 800.0, height - 1.0],
                            &font,
                            factor,
                            &options,
                            TextAlign::Left,
                            px,
                        ))
                        .unwrap();
                    assert_eq!(shortened.line_count(), 12);
                    assert!(shortened.ellipsized());
                }
            }
        }
    }
}

/// Text-only fixtures must never request downloaded artwork.
struct NoImages;

impl RemoteSource for NoImages {
    /// Rejects any unexpected image request from a text-only screen.
    fn state(&self, _url: &str) -> RemoteImage {
        panic!("the text fixture must not request images")
    }
}

/// Paints an editable, shadowed label through the same engine path that exports caret data.
fn paint_edit_label(
    font: &RuntimeFontCatalog,
    metrics: TextMetrics,
    factor: f32,
    name: &str,
) -> (EngineFrame, Vec<UiNode>) {
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let registry = REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop()));
    let bytes = encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    let assets = Arc::new(RuntimeUiAssets::decode(&bytes).unwrap());
    let engine = FormEngine::new(
        assets,
        Catalog::default(),
        0,
        registry,
        Arc::new(NoImages),
        ImageWork::inline(),
    );
    let json = serde_json::json!({
        "namespace": "fixture",
        "screen": {
            "type": "edit_box", "size": [300, 100], "text_control": "value", "max_length": 64,
            "anchor_from": "top_left", "anchor_to": "top_left",
            "controls": [{"value": {
                "type": "label", "text": "0000", "localize": false,
                "size": [300, 100], "anchor_from": "top_left", "anchor_to": "top_left",
                "offset": [8, 8], "font_scale_factor": factor, "font_type": name, "shadow": true
            }}]
        }
    })
    .to_string();
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", b"{}".as_slice()),
        (
            "ui/_ui_defs.json",
            br#"{"ui_defs":["ui/fixture.json"]}"#.as_slice(),
        ),
        ("ui/fixture.json", json.as_bytes()),
    ])
    .unwrap();
    let context = Context::desktop();
    let resolved = resolve(&catalog, "fixture.screen", &context)
        .control
        .expect("editable fixture resolves");
    let bound = bind_screen(
        &resolved,
        &catalog,
        &context,
        &DataSource::new(),
        &mut Default::default(),
    );
    let mut nodes = Vec::new();
    let mut next = 1;
    let frame = engine
        .draw(
            ScreenArt::default(),
            &mut PaintFrame {
                layouts: &mut TextLayoutCache::new(4, 1 << 20),
                font,
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1280.0, 720.0].map(|edge| edge / metrics.dpi_scale.get()),
                translate: &|_| None,
                language: [0; 3],
                now: 0.0,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut [],
            },
            |env, root| Some(render_bound(bound, root, env, &Default::default())),
        )
        .unwrap()
        .expect("editable label draws");
    (frame, nodes)
}

#[test]
fn named_edit_metadata_places_caret_at_the_painted_character_boundary() {
    let face = assets::carriers::FONT_TEN.font_face.unwrap();
    let alias = face.json_ui_alias.unwrap();
    let default = fixture_font();
    for em in [26, 52] {
        let font = default
            .with_named_font(face.name, &native_font(em))
            .unwrap();
        for name in [alias, "default", "unavailable"] {
            for dpi in [1.0, 1.5] {
                for factor in [0.5, 1.0, 2.0] {
                    let metrics = TextMetrics::for_viewport(
                        [1280, 720],
                        DpiScale::new(dpi).unwrap(),
                        Some(2),
                    );
                    let (frame, nodes) = paint_edit_label(&font, metrics, factor, name);
                    let [edit] = frame.edit_texts.as_slice() else {
                        panic!("one edit box");
                    };
                    let (node, layout) = nodes
                        .iter()
                        .find_map(|node| match node.visual() {
                            UiVisual::Text { layout, .. } => Some((node, layout)),
                            _ => None,
                        })
                        .expect("painted label");
                    let spot = TextSpot {
                        field: (),
                        bounds: node.bounds(),
                        left: edit.left,
                        factor: edit.scale,
                        font: edit.font.clone(),
                        letter_spacing_64: 0,
                        metrics,
                    };
                    let mut layouts = TextLayoutCache::new(4, 1 << 20);
                    let width = spot.width(&mut layouts, &font, "0000");
                    assert_eq!(width, layout.size_64()[0] as f32 / 64.0);
                    let x = layout.glyphs()[2].bounds_64[0] as f32 / 64.0;
                    let caret = nearest_boundary("0000", x, |prefix| {
                        spot.width(&mut layouts, &font, prefix)
                    });
                    assert_eq!(caret, 2, "em {em}, DPI {dpi}, factor {factor}, face {name}");
                }
            }
        }
    }
}

#[test]
fn named_label_shadow_keeps_its_display_offset_across_raster_sizes() {
    let face = assets::carriers::FONT_TEN.font_face.unwrap();
    let alias = face.json_ui_alias.unwrap();
    for em in [26, 52] {
        let default = fixture_font();
        let font = default
            .with_named_font(face.name, &native_font(em))
            .unwrap();
        for name in [alias, "default", "unavailable"] {
            for dpi in [1.0, 1.5] {
                for factor in [0.5, 1.0, 2.0] {
                    let metrics = TextMetrics::for_viewport(
                        [1280, 720],
                        DpiScale::new(dpi).unwrap(),
                        Some(2),
                    );
                    let (_, nodes) = paint_edit_label(&font, metrics, factor, name);
                    let viewport = rect(0.0, 0.0, 1280.0, 720.0).unwrap();
                    let mut tree = UiTree::new(nodes).unwrap();
                    tree.layout(viewport, UiScale::default(), SafeArea::ZERO)
                        .unwrap();
                    let draw = tree.build_draw_list().unwrap();
                    assert_eq!(draw.vertices.len(), 32, "four shadowed glyphs");
                    let (shadow, ink) = draw.vertices.split_at(16);
                    let expected =
                        ui::TEXT_SHADOW_OFFSET_64 as f32 / 64.0 * metrics.scale.get() * factor;
                    for (shadow, ink) in shadow.iter().zip(ink) {
                        for axis in 0..2 {
                            let offset = shadow.position[axis] - ink.position[axis];
                            assert!(
                                (offset - expected).abs() <= 1.0 / 32.0,
                                "em {em}, DPI {dpi}, factor {factor}, face {name}: {offset} != {expected}"
                            );
                        }
                    }
                }
            }
        }
    }
}
