//! The production painter retains catalogs, layouts and text across unchanged editor frames.

use super::*;

struct NoImages;
impl ui_engine::sources::RemoteSource for NoImages {
    /// The original editor fixture has no remote artwork.
    fn state(&self, _: &str) -> ui_engine::sources::RemoteImage {
        ui_engine::sources::RemoteImage::Failed
    }
}

/// Creates a production painter without local game carriers or a graphics device.
fn engine() -> FormEngine {
    use ui_engine::{engine::screen::ScreenRegistry, sources::ImageWork};
    static REGISTRY: std::sync::OnceLock<ScreenRegistry> = std::sync::OnceLock::new();
    let bytes = assets::encode_ui_catalog([1; 32], &[], &[], &[], &[]).unwrap();
    FormEngine::new(
        Arc::new(assets::RuntimeUiAssets::decode(&bytes).unwrap()),
        Catalog::default(),
        0,
        REGISTRY.get_or_init(|| ScreenRegistry::new(Context::desktop())),
        Arc::new(NoImages),
        ImageWork::inline(),
    )
}

/// Paints one editor frame through the same path used by the client.
fn paint(
    ui: &mut ModUi,
    engine: &FormEngine,
    font: &assets::RuntimeFontCatalog,
    layouts: &mut ui::TextLayoutCache,
    now: f64,
) {
    let icons = ui.mod_hud_editor_icons(|_, _| None).unwrap();
    ui.paint_mod_hud_editor(
        Some(engine),
        &mut PaintFrame {
            layouts,
            font,
            metrics: ui_engine::text_metrics::TextMetrics::for_viewport(
                [1280, 720],
                ui::DpiScale::new(1.).unwrap(),
                Some(2),
            ),
            solid_page: 0,
            safe_area: ui::SafeArea::ZERO,
            content: [1280., 720.],
            translate: &|_| None,
            language: [0; 3],
            now,
            nodes: &mut Vec::new(),
            next: &mut 1,
            renderers: &mut [],
        },
        &icons,
    );
    assert!(ui.mod_hud_editor_open());
}

#[test]
fn amendment_hidden_artwork_and_steady_frames_retain_catalog_bindings_and_text() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = ModUi::default();
    ui.set_mod_panel(
        crate::PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    let mut hud = Hud {
        inspector_size: Some([180., 120.]),
        cards: vec![ui::mod_hud::Card {
            id: "disabled".into(),
            title: "Hidden module".into(),
            visible: false,
            ..Default::default()
        }],
        ..Default::default()
    };
    ui.open_mod_hud_editor(&hud).unwrap();
    ui.mod_hud_editor.as_mut().unwrap().selected = Some(0);
    paint(&mut ui, &engine, &font, &mut layouts, 1.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert!(
        !editor.screen.nodes().iter().any(|node| matches!(&node.draw,
        json_ui::Draw::Text { text, .. } if text == "Hidden module"))
    );
    let catalog = Arc::clone(editor.catalog.as_ref().unwrap());
    let data = Arc::clone(&editor.data);
    let passes = editor.screen.passes;
    let shaped = layouts.built_layout_count();
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert!(Arc::ptr_eq(&catalog, editor.catalog.as_ref().unwrap()));
    assert!(Arc::ptr_eq(&data, &editor.data));
    assert_eq!(editor.screen.passes, passes);
    assert_eq!(layouts.built_layout_count(), shaped);
    for action in ["hud.grid", "hud.snap", "hud.visibility:0"] {
        ui.mod_hud_editor.as_mut().unwrap().action(action, &mut []);
        paint(&mut ui, &engine, &font, &mut layouts, 3.);
        assert!(Arc::ptr_eq(
            &catalog,
            ui.mod_hud_editor
                .as_ref()
                .unwrap()
                .catalog
                .as_ref()
                .unwrap()
        ));
    }
    assert!(
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .screen
            .nodes()
            .iter()
            .any(|node| matches!(&node.draw,
        json_ui::Draw::Text { text, .. } if text == "Hidden module"))
    );
    hud.inspector_size = Some([220., 160.]);
    ui.update_mod_hud_editor(&hud).unwrap();
    paint(&mut ui, &engine, &font, &mut layouts, 4.);
    assert!(Arc::ptr_eq(
        &catalog,
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap()
    ));
}

/// A short distance must not let its wider annotation cover the selected widget or nearby keys.
#[test]
fn measurement_captions_keep_selected_and_neighboring_artwork_clear() {
    let mut ui = ModUi::default();
    ui.set_mod_panel(
        crate::PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    let hud: Hud = serde_json::from_value(serde_json::json!({"cards":[
        {"id":"fps","anchor":"top_left","offset":[12,37],"width":56,"row_height":18,"rows":[{"label":"FPS","value":"144"}]},
        {"id":"keys","anchor":"top_left","offset":[0,60],"width":70,"row_height":26,"rows":[{"label":"W","value":""},{"label":"A S D","value":""},{"label":"SPACE","value":""}]}
    ]})).unwrap();
    ui.open_mod_hud_editor(&hud).unwrap();
    let editor = ui.mod_hud_editor.as_mut().unwrap();
    editor.viewport = [400., 240.];
    editor.selected = Some(0);
    let data = editor.data();
    for axis in 0..2 {
        assert_eq!(
            data.global_value(&format!("#measurement_{axis}_visible")),
            Some(&Scalar::Bool(true))
        );
        let Scalar::Json(value) = data
            .global_value(&format!("#measurement_{axis}_pill"))
            .unwrap()
        else {
            panic!("caption position")
        };
        let point: [f64; 2] = serde_json::from_value(value.clone()).unwrap();
        for (index, card) in editor.draft.cards.iter().enumerate() {
            let at = editor.card_origin(index);
            let size = cards::dimensions(card);
            assert!(
                point[0] + feedback::pill_size(if axis == 0 { "12" } else { "37" })[0] <= at[0]
                    || point[0] >= at[0] + size[0]
                    || point[1] + MEASUREMENT_PILL_HEIGHT <= at[1]
                    || point[1] >= at[1] + size[1],
                "caption covers {}",
                card.id
            );
        }
    }
}

/// Authored editor typography stays centered inside the distance pill rather than filling its band.
#[test]
fn compact_editor_typography_centers_measurements_in_their_backgrounds() {
    let mut ui = ModUi::default();
    ui.set_mod_panel(
        crate::PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    let hud: Hud = serde_json::from_str(r#"{"editor_style":{"font":"sans","text_scale":0.5,"measurement":[0.1,0.2,0.3,1]},"cards":[{"id":"fps","width":56,"offset":[37,41],"rows":[{"label":"FPS","value":"144"}]}]}"#).unwrap();
    ui.open_mod_hud_editor(&hud).unwrap();
    ui.mod_hud_editor.as_mut().unwrap().selected = Some(0);
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let font = font
        .with_named_font(ui::mod_panel::SANS_FONT_NAME, &font)
        .unwrap();
    paint(
        &mut ui,
        &engine,
        &font,
        &mut ui::TextLayoutCache::new(64, 1 << 24),
        1.,
    );
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    let mut labels = 0;
    for node in editor.screen.nodes() {
        if let json_ui::Draw::Text {
            text,
            scale,
            options,
            ..
        } = &node.draw
            && node.key.contains("measurement_")
            && node.key.contains("label")
            && !text.is_empty()
        {
            labels += 1;
            assert_eq!(*scale, MEASUREMENT_FONT_SCALE as f32);
            assert_eq!(
                options.font_type.as_deref(),
                Some(ui::mod_panel::SANS_FONT_NAME)
            );
            assert!(node.dest.h < MEASUREMENT_PILL_HEIGHT);
            assert!(
                node.dest.y >= node.clip.y
                    && node.dest.y + node.dest.h <= node.clip.y + node.clip.h
            );
        }
    }
    assert_eq!(labels, 2);
}

/// Native selection chrome reserves its full measured caption before placing the inspector and guides.
#[test]
fn measured_selection_caption_stays_compact_and_clear_of_the_inspector() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let font = font
        .with_named_font(ui::mod_panel::SANS_FONT_NAME, &font)
        .unwrap();
    for width in [56., 130., 400.] {
        for scale in [0.25, 1.] {
            let hud: Hud = serde_json::from_value(serde_json::json!({
                "editor_style":{"selection_caption":true,"font":"sans","text_scale":0.5,"top_clearance":44},
                "inspector_size":[104,204],
                "cards":[{"id":"fps","editor_label":"FPS","width":width,"scale":scale,"offset":[12,37],"rows":[{"label":"","value":"144 FPS"}]}]
            })).unwrap();
            let mut ui = ModUi::default();
            ui.set_mod_panel(
                crate::PanelUiOwner::Environment(0),
                Some(&crate::test_support::sample_panel()),
            )
            .unwrap();
            ui.set_mod_panel_open(true);
            ui.open_mod_hud_editor(&hud).unwrap();
            ui.mod_hud_editor.as_mut().unwrap().selected = Some(0);
            paint(
                &mut ui,
                &engine,
                &font,
                &mut ui::TextLayoutCache::new(64, 1 << 24),
                1.,
            );
            let editor = ui.mod_hud_editor.as_ref().unwrap();
            let caption = editor.caption_bounds.unwrap();
            assert_eq!(
                caption[2], 64.,
                "caption followed widget width {width} at scale {scale}"
            );
            let inspector = [editor.inspector.at[0], editor.inspector.at[1], 104., 204.];
            let overlaps = |a: [f64; 4], b: [f64; 4]| {
                a[0] < b[0] + b[2] && a[0] + a[2] > b[0] && a[1] < b[1] + b[3] && a[1] + a[3] > b[1]
            };
            assert!(
                !overlaps(caption, inspector),
                "inspector covers the caption"
            );
            assert!(editor.screen.nodes().iter().any(|node| node.key.contains("selection_caption") && matches!(&node.draw,json_ui::Draw::Custom { data,.. } if data.get("radius").and_then(serde_json::Value::as_f64)==Some(3.5)) && node.dest.w == 64.));
            for axis in 0..2 {
                if editor
                    .data
                    .global_value(&format!("#measurement_{axis}_visible"))
                    != Some(&Scalar::Bool(true))
                {
                    continue;
                }
                let Scalar::Json(value) = editor
                    .data
                    .global_value(&format!("#measurement_{axis}_pill"))
                    .unwrap()
                else {
                    panic!("caption position")
                };
                let point: [f64; 2] = serde_json::from_value(value.clone()).unwrap();
                let Some(Scalar::Num(width)) = editor
                    .data
                    .global_value(&format!("#measurement_{axis}_pill_width"))
                else {
                    panic!("pill width")
                };
                let pill = [point[0], point[1], *width, MEASUREMENT_PILL_HEIGHT];
                assert!(!overlaps(pill, caption) && !overlaps(pill, inspector));
                assert!(pill[1] >= 44., "measurement overlaps the authored toolbar");
            }
        }
    }
}

#[test]
fn guest_slider_fill_follows_panel_values_without_rebuilding_or_churning_data() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = ModUi::default();
    let mut panel = crate::test_support::sample_panel();
    let index = panel
        .controls
        .iter()
        .position(|c| matches!(c, ui::mod_panel::Control::Slider { .. }))
        .unwrap();
    let fill = format!("#row_{index}_fill");
    ui.set_mod_panel(crate::PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    ui.set_mod_panel_open(true);
    ui.open_mod_hud_editor(&Hud::default()).unwrap();
    paint(&mut ui, &engine, &font, &mut layouts, 1.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert_eq!(
        editor.data.global_value(&fill),
        Some(&Scalar::Num(f64::from(0.35_f32)))
    );
    let catalog = Arc::clone(editor.catalog.as_ref().unwrap());
    let data = Arc::clone(&editor.data);
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    assert!(Arc::ptr_eq(
        &data,
        &ui.mod_hud_editor.as_ref().unwrap().data
    ));
    if let ui::mod_panel::Control::Slider { value, .. } = &mut panel.controls[index] {
        *value = 80.;
    }
    // A same-shape guest republish keeps the editor open and only moves the fill.
    ui.set_mod_panel(crate::PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    assert!(ui.mod_hud_editor_open());
    paint(&mut ui, &engine, &font, &mut layouts, 3.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert_eq!(
        editor.data.global_value(&fill),
        Some(&Scalar::Num(f64::from(0.8_f32)))
    );
    assert!(Arc::ptr_eq(&catalog, editor.catalog.as_ref().unwrap()));
    let data = Arc::clone(&editor.data);
    paint(&mut ui, &engine, &font, &mut layouts, 4.);
    assert!(Arc::ptr_eq(
        &data,
        &ui.mod_hud_editor.as_ref().unwrap().data
    ));
}

/// Opens an editor over `cards` and paints its first frame.
fn painted(
    ui: &mut ModUi,
    engine: &FormEngine,
    font: &assets::RuntimeFontCatalog,
    layouts: &mut ui::TextLayoutCache,
    cards: Vec<ui::mod_hud::Card>,
) {
    ui.set_mod_panel(
        crate::PanelUiOwner::Environment(0),
        Some(&crate::test_support::sample_panel()),
    )
    .unwrap();
    ui.set_mod_panel_open(true);
    ui.open_mod_hud_editor(&Hud {
        cards,
        ..Default::default()
    })
    .unwrap();
    paint(ui, engine, font, layouts, 1.);
}

/// Distance pills carry a bare number and stay inside the viewport when clamped at either edge.
#[test]
fn distance_pills_show_bare_numbers_and_fit_at_viewport_edges() {
    let viewport = [640., 360.];
    for rect in [
        ([246., 0.], [148., 4.]),
        ([246., 356.], [148., 4.]),
        ([0., 100.], [30., 20.]),
        ([610., 100.], [30., 20.]),
        ([300., 160.], [40., 40.]),
    ] {
        let mut data = DataSource::new();
        feedback::measurements(&mut data, viewport, Some(rect), &[], 1.);
        for axis in 0..2 {
            let global = |suffix: &str| {
                data.global_value(&format!("#measurement_{axis}_{suffix}"))
                    .cloned()
                    .unwrap()
            };
            let Scalar::Bool(visible) = global("visible") else {
                panic!("visibility")
            };
            let Scalar::Text(label) = global("label") else {
                panic!("label")
            };
            if !visible {
                continue;
            }
            assert!(
                !label.is_empty() && label.bytes().all(|b| b.is_ascii_digit()),
                "{label:?} must be a bare number"
            );
            let Scalar::Num(width) = global("pill_width") else {
                panic!("pill width")
            };
            let Scalar::Json(pill) = global("pill") else {
                panic!("pill offset")
            };
            let size = [width, MEASUREMENT_PILL_HEIGHT];
            assert!(width <= 20., "{label} pill is {width} px wide");
            for (n, extent) in size.into_iter().enumerate() {
                let at = pill[n].as_f64().unwrap();
                assert!(
                    at >= 0. && at + extent <= viewport[n],
                    "{rect:?} axis {axis}"
                );
            }
        }
    }
}

/// The painted label is centred inside its pill at the compact font scale.
#[test]
fn distance_pill_label_paints_inside_its_background() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = ModUi::default();
    let card = ui::mod_hud::Card {
        id: "fps".into(),
        position: Some([0.5, 0.]),
        ..Default::default()
    };
    painted(&mut ui, &engine, &font, &mut layouts, vec![card]);
    ui.mod_hud_editor.as_mut().unwrap().selected = Some(0);
    ui.mod_hud_editor.as_mut().unwrap().data_dirty = true;
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    let Some(Scalar::Text(label)) = editor.data.global_value("#measurement_0_label") else {
        panic!("label")
    };
    let measurement = ui::mod_hud::EditorStyle::default()
        .measurement
        .map(|v| (v * 255.).round() as u8);
    let nodes = editor.screen.nodes();
    // Node rectangles are in paint units; the default card surface is 148 GUI px wide.
    let px = nodes
        .iter()
        .find(|node| node.key.ends_with("card_0/surface"))
        .expect("card surface")
        .dest
        .w
        / f64::from(ui::mod_hud::DEFAULT_CARD_WIDTH);
    let background = nodes
        .iter()
        .find(|node| node.key.contains("measurement_0_pill") && matches!(&node.draw, json_ui::Draw::Custom { data, .. }
            if data.get("#color").or(data.get("color")).and_then(json_ui::color_value) == Some(measurement)))
        .expect("pill background");
    let (text, scale) = nodes
        .iter()
        .find_map(|node| match &node.draw {
            json_ui::Draw::Text { text, scale, .. } if text == label => Some((node, *scale)),
            _ => None,
        })
        .expect("pill label");
    let [bx, by, bw, bh] = [
        background.dest.x,
        background.dest.y,
        background.dest.w,
        background.dest.h,
    ];
    assert!(
        (bh / px - MEASUREMENT_PILL_HEIGHT).abs() < 0.01,
        "pill height {}",
        bh / px
    );
    assert!(
        (bw / px - feedback::pill_size(label)[0]).abs() < 0.01,
        "pill width {}",
        bw / px
    );
    assert!(bx >= 0. && by >= 0.);
    let center = [
        text.dest.x + text.dest.w * 0.5,
        text.dest.y + text.dest.h * 0.5,
    ];
    assert!((center[0] - (bx + bw * 0.5)).abs() < 0.5);
    assert!((center[1] - (by + bh * 0.5)).abs() < 0.5);
    assert!(text.dest.h <= bh);
    assert!(
        (scale - MEASUREMENT_FONT_SCALE as f32).abs() < 0.01,
        "font scale {scale}"
    );
}

/// Outlines appear only on hover or selection, through data bindings rather than a rebuild.
#[test]
fn card_outlines_follow_hover_and_selection_without_rebuilding() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = ModUi::default();
    let cards = vec![
        ui::mod_hud::Card {
            id: "a".into(),
            position: Some([0.1, 0.1]),
            background_radius: 8.,
            ..Default::default()
        },
        ui::mod_hud::Card {
            id: "b".into(),
            position: Some([0.9, 0.9]),
            ..Default::default()
        },
    ];
    painted(&mut ui, &engine, &font, &mut layouts, cards);
    let style = ui::mod_hud::EditorStyle::default();
    let color = |ui: &ModUi, index: usize| match ui
        .mod_hud_editor
        .as_ref()
        .unwrap()
        .data
        .global_value(&format!("#bounds_{index}_color"))
    {
        Some(Scalar::Json(value)) => serde_json::from_value::<[f32; 4]>(value.clone()).unwrap(),
        other => panic!("bounds colour {other:?}"),
    };
    assert_eq!(color(&ui, 0), [0.; 4]);
    assert_eq!(color(&ui, 1), [0.; 4]);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert!(
        editor.screen.nodes().iter().any(|node| matches!(&node.draw,
            json_ui::Draw::Custom { data, .. } if data.get("radius").and_then(serde_json::Value::as_f64) == Some(8.))),
        "preview background keeps its authored radius"
    );
    let catalog = Arc::clone(editor.catalog.as_ref().unwrap());
    let frame = editor.frame.as_ref().unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
        .unwrap();
    let at = [
        (hit.rect.x + hit.rect.w * 0.5) as f32 * frame.scale + frame.origin[0],
        (hit.rect.y + hit.rect.h * 0.5) as f32 * frame.scale + frame.origin[1],
    ];
    ui.mod_hud_editor
        .as_mut()
        .unwrap()
        .pointer(at, false, false, &mut []);
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    assert_eq!(color(&ui, 0), style.outline);
    assert_eq!(color(&ui, 1), [0.; 4]);
    let editor = ui.mod_hud_editor.as_mut().unwrap();
    assert!(Arc::ptr_eq(&catalog, editor.catalog.as_ref().unwrap()));
    editor.selected = Some(1);
    editor.data_dirty = true;
    paint(&mut ui, &engine, &font, &mut layouts, 3.);
    assert_eq!(color(&ui, 1), style.selected);
    assert!(Arc::ptr_eq(
        &catalog,
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap()
    ));
}

/// A hovered text plate keeps its outline through live remeasurement and template rebuilds.
#[test]
fn hovered_outline_survives_text_plate_remeasurement_and_rebuilds() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = ModUi::default();
    let mut hud: Hud = serde_json::from_value(serde_json::json!({"cards":[{"id":"fps",
        "position":[0.5,0.5],"rows":[{"label":"","value":"1"}],"text_plate":{}}]}))
    .unwrap();
    painted(&mut ui, &engine, &font, &mut layouts, hud.cards.clone());
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    let frame = editor.frame.as_ref().unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
        .unwrap();
    let at = [
        (hit.rect.x + hit.rect.w * 0.5) as f32 * frame.scale + frame.origin[0],
        (hit.rect.y + hit.rect.h * 0.5) as f32 * frame.scale + frame.origin[1],
    ];
    ui.mod_hud_editor
        .as_mut()
        .unwrap()
        .pointer(at, false, false, &mut []);
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    let outline = || {
        ui::mod_hud::EditorStyle::default()
            .outline
            .map(|v| serde_json::json!(v))
    };
    let bound = |ui: &ModUi| {
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .data
            .global_value("#bounds_0_color")
            .cloned()
    };
    assert_eq!(bound(&ui), Some(Scalar::Json(serde_json::json!(outline()))));
    hud.cards[0].rows[0].value = "123456".into();
    ui.update_mod_hud_editor(&hud).unwrap();
    paint(&mut ui, &engine, &font, &mut layouts, 3.);
    assert_eq!(bound(&ui), Some(Scalar::Json(serde_json::json!(outline()))));
    let catalog = Arc::clone(
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .catalog
            .as_ref()
            .unwrap(),
    );
    hud.cards[0].text_plate.as_mut().unwrap().style.radius = 4.;
    ui.update_mod_hud_editor(&hud).unwrap();
    paint(&mut ui, &engine, &font, &mut layouts, 4.);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    assert!(!Arc::ptr_eq(&catalog, editor.catalog.as_ref().unwrap()));
    assert_eq!(bound(&ui), Some(Scalar::Json(serde_json::json!(outline()))));
}

/// A card replaced under a still pointer is outlined on the next pointer event over the same key.
#[test]
fn hovered_outline_follows_a_card_replaced_under_the_pointer() {
    let engine = engine();
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = ui::TextLayoutCache::new(64, 1 << 24);
    let mut ui = ModUi::default();
    let card = |id: &str| ui::mod_hud::Card {
        id: id.into(),
        position: Some([0.5, 0.5]),
        ..Default::default()
    };
    painted(&mut ui, &engine, &font, &mut layouts, vec![card("a")]);
    let editor = ui.mod_hud_editor.as_ref().unwrap();
    let frame = editor.frame.as_ref().unwrap();
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.pressed.as_deref() == Some("hud.card:0"))
        .unwrap();
    let at = [
        (hit.rect.x + hit.rect.w * 0.5) as f32 * frame.scale + frame.origin[0],
        (hit.rect.y + hit.rect.h * 0.5) as f32 * frame.scale + frame.origin[1],
    ];
    let hover = |ui: &mut ModUi| {
        ui.mod_hud_editor
            .as_mut()
            .unwrap()
            .pointer(at, false, false, &mut [])
    };
    hover(&mut ui);
    paint(&mut ui, &engine, &font, &mut layouts, 2.);
    ui.update_mod_hud_editor(&Hud {
        cards: vec![card("b")],
        ..Default::default()
    })
    .unwrap();
    paint(&mut ui, &engine, &font, &mut layouts, 3.);
    hover(&mut ui);
    paint(&mut ui, &engine, &font, &mut layouts, 4.);
    assert_eq!(
        ui.mod_hud_editor
            .as_ref()
            .unwrap()
            .data
            .global_value("#bounds_0_color"),
        Some(&Scalar::Json(serde_json::json!(
            ui::mod_hud::EditorStyle::default().outline
        )))
    );
}
