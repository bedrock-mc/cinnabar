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
fn minecraftten_alias_normalizes_outline_line_height_across_rasters() {
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
                let native_geometry = line_geometry(&native);
                let source = font.font_named(alias).line_metrics().unwrap();
                let height = (source.ascent_64 + source.descent_64) as f32;
                let expected = [
                    source.em_64 as f32 / height * OUTLINE_LABEL_HEIGHT_GUI,
                    OUTLINE_LABEL_HEIGHT_GUI,
                ]
                .map(|value| (value * factor * px * 64.0).round() as i32);
                for (expected, actual) in expected.into_iter().zip(native_geometry) {
                    // Scale and glyph edges have fixed-point rounding at each raster size.
                    assert!(
                        (expected - actual).abs() <= 5,
                        "em {em}, DPI {dpi}, factor {factor}: {expected} != {actual}"
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
    text: &str,
) -> (EngineFrame, Vec<UiNode>) {
    let json = serde_json::json!({
        "namespace": "fixture",
        "screen": {
            "type": "edit_box", "size": [300, 100], "text_control": "value", "max_length": 64,
            "anchor_from": "top_left", "anchor_to": "top_left",
            "controls": [{"value": {
                "type": "label", "text": text, "localize": false,
                "size": [300, 100], "anchor_from": "top_left", "anchor_to": "top_left",
                "offset": [8.25, 8.25], "font_scale_factor": factor, "font_type": name, "shadow": true
            }}]
        }
    })
    .to_string();
    paint_fixture(font, metrics, &json)
}

/// Resolves, lays out and paints an original text-only screen through the owning engine.
fn paint_fixture(
    font: &RuntimeFontCatalog,
    metrics: TextMetrics,
    json: &str,
) -> (EngineFrame, Vec<UiNode>) {
    paint_fixture_selection(font, metrics, json, None)
}

#[test]
fn long_form_label_paints_a_visible_prefix() {
    let text = "0".repeat(ui::UiLimits::MAX_TEXT_BYTES + 4_096);
    let json = serde_json::json!({"namespace":"fixture", "screen":{
        "type":"screen", "controls":[{"form_label":{
            "type":"label", "text":text, "localize":false, "size":[300,100],
            "anchor_from":"top_left", "anchor_to":"top_left"
        }}]
    }})
    .to_string();
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), Some(2));
    let (_, nodes) = paint_fixture(&fixture_font(), metrics, &json);
    assert!(nodes.iter().any(|node| matches!(node.visual(),
        UiVisual::Text { layout, .. } if layout.glyphs().iter().any(|glyph| glyph.codepoint == '0')
    )), "the form label must paint part of its text");
    let mut tree = UiTree::new(nodes).unwrap();
    tree.layout(
        rect(0.0, 0.0, 1280.0, 720.0).unwrap(),
        UiScale::default(),
        SafeArea::ZERO,
    )
    .unwrap();
    assert!(!tree.build_draw_list().unwrap().vertices.is_empty());
}

/// Paints a focused root edit box with the host's actual selection feedback.
fn paint_fixture_selection(
    font: &RuntimeFontCatalog,
    metrics: TextMetrics,
    json: &str,
    selection: Option<[usize; 2]>,
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
    let view = json_ui::ViewState {
        focused: selection.map(|_| format!("/{}", resolved.name)),
        ..Default::default()
    };
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
            ScreenArt {
                view: selection.map(|_| &view),
                edit: selection.map(|range| super::super::host_edit::Feedback::new(Some(range))),
                ..Default::default()
            },
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
            |env, root| {
                let render = render_bound(bound, root, env, &view);
                if let Some(key) = view.focused.as_deref() {
                    let edit = render
                        .hits
                        .iter()
                        .find(|hit| hit.key == key)
                        .and_then(|hit| hit.widget.edit.as_ref())
                        .expect("focus resolves to the laid-out edit box");
                    assert!(edit.text_target.is_some(), "the focused edit owns a label");
                }
                Some(render)
            },
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
                    let (frame, nodes) = paint_edit_label(&font, metrics, factor, name, "0000");
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
                    assert_eq!(
                        edit.left,
                        node.bounds().min().x(),
                        "caret and painted text use the same fractional origin"
                    );
                    let spot = TextSpot {
                        layout: None,
                        field: (),
                        bounds: node.bounds(),
                        left: edit.left,
                        factor: edit.scale,
                        effect_scale_1024: edit.effect_scale_1024,
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
                    let (_, nodes) = paint_edit_label(&font, metrics, factor, name, "0000");
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

#[test]
fn named_styled_font_effects_use_display_units_at_every_raster_resolution() {
    let face = assets::carriers::FONT_TEN.font_face.unwrap();
    let alias = face.json_ui_alias.unwrap();
    for em in [26, 52] {
        let font = fixture_font()
            .with_named_font(face.name, &native_font(em))
            .unwrap();
        for dpi in [1.0, 1.5] {
            for factor in [1.0, 1.3] {
                let metrics =
                    TextMetrics::for_viewport([1280, 720], DpiScale::new(dpi).unwrap(), Some(2));
                let text = "§l§o0000";
                let (frame, nodes) = paint_edit_label(&font, metrics, factor, alias, text);
                let [edit] = frame.edit_texts.as_slice() else {
                    panic!("one edit field")
                };
                let (node, layout) = nodes
                    .iter()
                    .find_map(|node| match node.visual() {
                        UiVisual::Text { layout, .. } => Some((node, Arc::clone(layout))),
                        _ => None,
                    })
                    .unwrap();
                let spot = TextSpot {
                    layout: None,
                    field: (),
                    bounds: node.bounds(),
                    left: edit.left,
                    factor: edit.scale,
                    effect_scale_1024: edit.effect_scale_1024,
                    font: edit.font.clone(),
                    letter_spacing_64: 0,
                    metrics,
                };
                assert_eq!(
                    spot.width(&mut TextLayoutCache::new(4, 1 << 20), &font, text),
                    layout.size_64()[0] as f32 / 64.0
                );
                let mut tree = UiTree::new(nodes).unwrap();
                tree.layout(
                    rect(0.0, 0.0, 1280.0, 720.0).unwrap(),
                    UiScale::default(),
                    SafeArea::ZERO,
                )
                .unwrap();
                let draw = tree.build_draw_list().unwrap();
                assert_eq!(draw.vertices.len(), 64, "four bold glyphs with shadow");
                let ink = &draw.vertices[32..];
                let (_, reference_nodes) =
                    paint_edit_label(&fixture_font(), metrics, factor, "default", text);
                let mut reference = UiTree::new(reference_nodes).unwrap();
                reference
                    .layout(
                        rect(0.0, 0.0, 1280.0, 720.0).unwrap(),
                        UiScale::default(),
                        SafeArea::ZERO,
                    )
                    .unwrap();
                let reference = reference.build_draw_list().unwrap();
                let reference = &reference.vertices[32..];
                let expected_bold = reference[4].position[0] - reference[0].position[0];
                let expected_italic = reference[0].position[0] - reference[3].position[0];
                for glyph in ink.as_chunks::<8>().0 {
                    let bold = glyph[4].position[0] - glyph[0].position[0];
                    let italic = glyph[0].position[0] - glyph[3].position[0];
                    assert!(
                        (bold - expected_bold).abs() < 0.02,
                        "em {em}, DPI {dpi}, factor {factor}: bold {bold}"
                    );
                    assert!(
                        (italic - expected_italic).abs() < 0.02,
                        "em {em}, DPI {dpi}, factor {factor}: italic {italic}"
                    );
                    let pen = (layout.glyphs()[1].bounds_64[0] - layout.glyphs()[0].bounds_64[0])
                        as f32
                        / 64.0;
                    let width = (layout.glyphs()[0].bounds_64[2] - layout.glyphs()[0].bounds_64[0])
                        as f32
                        / 64.0;
                    assert!(
                        (pen - width - bold).abs() < 0.02,
                        "bold paint and pen advance must agree"
                    );
                }
            }
        }
    }
}

/// Original cap rectangles expose label sizing and baseline placement without external art.
fn cap_font(em: u16) -> RuntimeFontCatalog {
    let cap = (u32::from(em) * 7 / 10) as u16;
    let pixels: Arc<[u8]> = vec![255; usize::from(em) * usize::from(cap)].into();
    let page = FontTexturePage {
        source_path: "font/original-cap.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [3; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: u32::from(em),
        height: u32::from(cap),
        pixels: FontPixels::Coverage(pixels),
    };
    let source = assets::carriers::FONT_TEN
        .font_face
        .unwrap()
        .line_metrics_at(u32::from(em));
    let glyphs = ['0', '\u{fffd}'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [0, 0, em, cap],
        bearing: [0, -(cap as i16)],
        advance_64: em as i16 * 64,
    });
    let bytes = encode_font_catalog([3; 32], &glyphs, &[page]).unwrap();
    RuntimeFontCatalog::decode(&bytes, [3; 32])
        .unwrap()
        .with_rendering(FontRendering::NativeSdf)
        .with_line_metrics(source)
        .unwrap()
}

#[test]
fn centered_outline_title_retains_cap_size_and_gui_pixel_inset() {
    let face = assets::carriers::FONT_TEN.font_face.unwrap();
    for em in [40, 80] {
        let font = fixture_font()
            .with_named_font(face.name, &cap_font(em))
            .unwrap();
        for gui in [2, 3] {
            for dpi in [1.0, 1.5] {
                let metrics =
                    TextMetrics::for_viewport([1920, 1080], DpiScale::new(dpi).unwrap(), Some(gui));
                assert_eq!(
                    metrics.gui_scale, gui as f32,
                    "exercise the requested GUI scale"
                );
                let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
                let json = serde_json::json!({
                    "namespace": "fixture",
                    "screen": { "type": "panel", "size": [200, 26],
                        "anchor_from": "top_left", "anchor_to": "top_left",
                        "controls": [{ "title": { "type": "label", "text": "0",
                            "localize": false, "font_type": face.json_ui_alias.unwrap(),
                            "anchor_from": "left_middle", "anchor_to": "left_middle",
                            "max_size": ["100%", 20], "font_scale_factor": 1.3,
                            "offset": [0, -2.5], "shadow": false } }]
                    }
                })
                .to_string();
                let (_, nodes) = paint_fixture(&font, metrics, &json);
                let mut tree = UiTree::new(nodes).unwrap();
                tree.layout(
                    rect(0.0, 0.0, 1280.0 / dpi, 720.0 / dpi).unwrap(),
                    UiScale::default(),
                    SafeArea::ZERO,
                )
                .unwrap();
                let draw = tree.build_draw_list().unwrap();
                assert_eq!(draw.vertices.len(), 4, "one original cap quad");
                let top = draw
                    .vertices
                    .iter()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::INFINITY, f32::min);
                let bottom = draw
                    .vertices
                    .iter()
                    .map(|vertex| vertex.position[1])
                    .fold(f32::NEG_INFINITY, f32::max);
                let ink_height = (bottom - top) / px;
                let center = (top + bottom) * 0.5 / px;
                assert!(
                    (ink_height - 8.8).abs() < 0.15,
                    "em {em}, GUI {gui}, DPI {dpi}: cap height {ink_height}"
                );
                assert!(
                    (center - 11.75).abs() < 0.15,
                    "em {em}, GUI {gui}, DPI {dpi}: title center {center}"
                );
            }
        }
    }
}

/// A full line-box rectangle includes a named font's descender region.
fn descender_font(em: u16) -> RuntimeFontCatalog {
    let face = assets::carriers::FONT_SEVEN.font_face.unwrap();
    let source = face.line_metrics_at(u32::from(em));
    let height = ((source.ascent_64 + source.descent_64) / 64) as u16;
    let pixels: Arc<[u8]> = vec![255; usize::from(em) * usize::from(height)].into();
    let page = FontTexturePage {
        source_path: "font/original-descender.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [4; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: u32::from(em),
        height: u32::from(height),
        pixels: FontPixels::Coverage(pixels),
    };
    let glyphs = ['0', '\u{fffd}'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [0, 0, em, height],
        bearing: [0, -(source.ascent_64 as i16 / 64)],
        advance_64: em as i16 * 64,
    });
    let bytes = encode_font_catalog([4; 32], &glyphs, &[page]).unwrap();
    RuntimeFontCatalog::decode(&bytes, [4; 32])
        .unwrap()
        .with_rendering(FontRendering::NativeSdf)
        .with_line_metrics(source)
        .unwrap()
}

#[test]
fn named_descender_selection_covers_the_painted_glyph() {
    let face = assets::carriers::FONT_SEVEN.font_face.unwrap();
    let font = fixture_font()
        .with_named_font(face.name, &descender_font(40))
        .unwrap();
    let json = serde_json::json!({ "namespace": "fixture", "screen": {
        "type": "edit_box", "size": [200, 100], "text_control": "value", "max_length": 64,
        "anchor_from": "top_left", "anchor_to": "top_left", "controls": [{ "value": {
            "type": "label", "text": "0", "localize": false, "size": [200, 100],
            "anchor_from": "top_left", "anchor_to": "top_left", "font_type": face.name,
            "offset": [8, 8], "shadow": false } }] } })
    .to_string();
    for dpi in [1.0, 1.5] {
        let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(dpi).unwrap(), Some(2));
        let (_, unfocused) = paint_fixture(&font, metrics, &json);
        assert!(
            unfocused
                .iter()
                .all(|node| !matches!(node.visual(), UiVisual::InvertedSprite { .. }))
        );
        let (_, nodes) = paint_fixture_selection(&font, metrics, &json, Some([0, 1]));
        let (bounds, layout) = nodes
            .iter()
            .find_map(|node| match node.visual() {
                UiVisual::Text { layout, .. } => Some((node.bounds(), layout)),
                _ => None,
            })
            .unwrap();
        let selection = nodes
            .iter()
            .find_map(|node| {
                matches!(node.visual(), UiVisual::InvertedSprite { .. }).then_some(node.bounds())
            })
            .expect("focused edit feedback paints a selection");
        let glyph = &layout.glyphs()[0];
        let top = bounds.min().y() + glyph.bounds_64[1] as f32 / 64.0;
        let bottom = bounds.min().y() + glyph.bounds_64[3] as f32 / 64.0;
        assert!(
            selection.min().y() <= top + 1.0 / 64.0,
            "selected glyph top remains covered"
        );
        assert!(
            selection.max().y() + 1.0 / 64.0 >= bottom,
            "DPI {dpi}: descender bottom {bottom} exceeds selection {}",
            selection.max().y()
        );
    }
}

/// Smooth outline faces retain fractional placement; pixel faces keep their physical grid.
#[test]
fn filtered_outline_labels_preserve_fractional_positions() {
    for (rendering, smooth) in [
        (FontRendering::NativeCoverage, false),
        (FontRendering::OutlineCoverage, true),
        (FontRendering::NativeSdf, true),
    ] {
        let face = native_font(26)
            .with_rendering(rendering)
            .with_linear_sampling();
        let font = fixture_font()
            .with_named_font("fixture_outline", &face)
            .unwrap();
        for dpi in [1., 1.5] {
            let metrics =
                TextMetrics::for_viewport([1280, 720], DpiScale::new(dpi).unwrap(), Some(2));
            let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
            let options = TextOptions {
                font_type: Some("fixture_outline".into()),
                ..Default::default()
            };
            let first = label_positioned(
                [8.125, 8.125, 100., 20.],
                &font,
                &options,
                metrics.gui_scale,
                px,
            );
            let next = label_positioned(
                [8.375, 8.375, 100., 20.],
                &font,
                &options,
                metrics.gui_scale,
                px,
            );
            let delta = next[0] - first[0];
            if smooth {
                assert!(
                    (delta - 0.25 * px).abs() < 0.0001,
                    "{rendering:?}, DPI {dpi}: {delta}"
                );
            } else {
                assert_eq!(delta, 0.);
            }
        }
    }
}
