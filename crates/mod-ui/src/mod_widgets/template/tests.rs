use super::*;

/// Center anchoring follows viewport and scale changes until a saved drag position overrides it.
#[test]
fn centered_compass_follows_viewport_and_saved_placement() {
    let mut card:Card=serde_json::from_str(r#"{"id":"direction","rows":[],"width":280,"offset":[0,8],"anchor":"top_center","compass":{}}"#).unwrap();
    assert_eq!(origin(&card, [800., 500.]), [260., 8.]);
    assert_eq!(origin(&card, [400., 200.]), [60., 8.]);
    card.scale = 2.;
    assert_eq!(origin(&card, [400., 200.]), [0., 8.]);
    card.scale = 1.;
    card.position = Some([0.25, 0.5]);
    assert_eq!(origin(&card, [400., 200.]), [30., 84.]);
}

/// An inventory has nine columns and retains all 36 slots, including empty ones.
#[test]
fn inventory_grid_geometry_preserves_slot_count() {
    let card = Card {
        id: "inventory".into(),
        width: 180.,
        row_height: 20.,
        icon_size: 16.,
        row_layout: RowLayout::Grid,
        rows: (0..36)
            .map(|_| ui::mod_hud::Row {
                label: String::new(),
                value: String::new(),
                item: None,
                effect_id: None,
                metadata: 0,
                progress: None,
                color: [1.; 4],
                localized: false,
                formatted: false,
            })
            .collect(),
        ..Default::default()
    };
    assert_eq!(dimensions(&card), [180., 84.]);
    let mut count = 0;
    let _ = grid(&card, &mut count, 0.);
    assert_eq!(count, 36);
}

/// A shadow setting changes the rendered template even when geometry stays identical.
#[test]
fn shadow_changes_invalidate_the_retained_template() {
    let mut a = Hud {
        cards: vec![Card {
            id: "armor".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let b = a.clone();
    a.cards[0].shadow = false;
    assert!(!same_shape(&a, &b));
}

struct CountArt;
impl json_ui::TextMeasure for CountArt {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.len() as f64 * 4., 8.]
    }
}
impl json_ui::TextureSource for CountArt {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Every row layout scales its rendered labels without resizing the card.
#[test]
fn row_text_scale_changes_rendered_labels_independently_of_card_scale() {
    let env = json_ui::LayoutEnv {
        text: &CountArt,
        textures: &CountArt,
    };
    let render_scales = |hud: &Hud| {
        let catalog = catalog(hud).unwrap();
        let root = json_ui::resolve(
            &catalog,
            "cinnabar_personal_hud.cards",
            &json_ui::Context::default(),
        )
        .control
        .unwrap();
        let bound = json_ui::bind(&root, &data(hud, [1600., 900.]), &json_ui::EmptyLibrary);
        let laid = json_ui::layout(&bound, [1600., 900.], &env);
        json_ui::emit(&laid, &env)
            .into_iter()
            .filter_map(|node| match node.draw {
                json_ui::Draw::Text { text, scale, .. } if !text.is_empty() => Some((text, scale)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    for row_layout in [
        RowLayout::Standard,
        RowLayout::StackedText,
        RowLayout::IconRight,
        RowLayout::Grid,
    ] {
        let label_count = match row_layout {
            RowLayout::Standard | RowLayout::StackedText | RowLayout::IconRight => 2,
            RowLayout::Grid => 1,
        };
        for scale in [ui::mod_hud::MIN_CARD_SCALE, 1., ui::mod_hud::MAX_CARD_SCALE] {
            let mut hud: Hud = serde_json::from_value(json!({"cards": [{
                "id": "inventory", "row_layout": row_layout, "scale": scale,
                "rows": [{"label": "Items", "value": "37"}]
            }]}))
            .unwrap();
            hud.validate().unwrap();
            let baseline = render_scales(&hud);
            assert_eq!(baseline.len(), label_count);
            let size = dimensions(&hud.cards[0]);
            for text_scale in [0.5, 2.] {
                hud.cards[0].text_scale = text_scale;
                let scaled = render_scales(&hud);
                assert_eq!(scaled.len(), baseline.len());
                assert_eq!(dimensions(&hud.cards[0]), size);
                for ((text, actual), (expected_text, base)) in scaled.iter().zip(&baseline) {
                    assert_eq!(text, expected_text);
                    assert!(
                        (actual - base * text_scale).abs() <= f32::EPSILON,
                        "{row_layout:?} at card scale {scale}, text scale {text_scale}: {text} has font scale {actual}, baseline {base}"
                    );
                }
            }
        }
    }
}

/// Count draw commands retain the current row tint and alpha without rebuilding the template.
#[test]
fn grid_count_labels_follow_live_row_tint_and_alpha() {
    let mut hud: Hud = serde_json::from_value(json!({"cards": [{
        "id": "inventory", "row_layout": "grid", "rows": [{"label": "", "value": "37"}]
    }]}))
    .unwrap();
    let catalog = catalog(&hud).unwrap();
    let root = json_ui::resolve(
        &catalog,
        "cinnabar_personal_hud.cards",
        &json_ui::Context::default(),
    )
    .control
    .unwrap();
    let env = json_ui::LayoutEnv {
        text: &CountArt,
        textures: &CountArt,
    };
    for color in [[0.2, 0.4, 0.6, 0.5], [0.8, 0.2, 0.4, 0.0]] {
        hud.cards[0].rows[0].color = color;
        let bound = json_ui::bind(&root, &data(&hud, [400., 200.]), &json_ui::EmptyLibrary);
        let laid = json_ui::layout(&bound, [400., 200.], &env);
        let draws = json_ui::emit(&laid, &env);
        let colors = draws
            .iter()
            .filter_map(|node| match &node.draw {
                json_ui::Draw::Text { text, color, .. } if text == "37" => Some(*color),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(colors, vec![json_ui::color_value(&json!(color)).unwrap()]);
    }
}

/// A selected card face reaches every label and changing it invalidates retained typography.
#[test]
fn card_font_selects_smooth_cell_labels_and_invalidates_geometry() {
    let native: Hud = serde_json::from_str(
        r#"{"cards":[{"id":"keys","rows":[],"cells":[{"rect":[0,0,22,22],"label":"W"}]}]}"#,
    )
    .unwrap();
    let mut sans = native.clone();
    sans.cards[0].font = ui::mod_hud::CardFont::Sans;
    assert!(!same_shape(&native, &sans));
    let mut index = 0;
    let root = card(&sans.cards[0], 0, &mut index);
    fn check(node: &Value, count: &mut usize) {
        match node {
            Value::Object(object) => {
                if object.get("type").and_then(Value::as_str) == Some("label") {
                    assert_eq!(
                        object.get("font_type").and_then(Value::as_str),
                        Some(ui::mod_panel::SANS_FONT_NAME)
                    );
                    *count += 1;
                }
                for value in object.values() {
                    check(value, count);
                }
            }
            Value::Array(values) => {
                for value in values {
                    check(value, count);
                }
            }
            _ => {}
        }
    }
    let mut count = 0;
    check(&root, &mut count);
    assert_eq!(count, 1);
    assert!(
        serde_json::from_str::<Hud>(r#"{"cards":[{"id":"bad","rows":[],"font":"unknown"}]}"#)
            .is_err()
    );
}
/// Positive edge offsets remain inward after a viewport resize.
#[test]
fn anchored_edge_offset_survives_viewport_change() {
    let card = ui::mod_hud::Card {
        anchor: ui::mod_hud::Anchor::BottomRight,
        offset: [13., 17.],
        ..Default::default()
    };
    let size = super::dimensions(&card);
    for viewport in [[640., 360.], [960., 540.]] {
        let at = super::origin(&card, viewport);
        for axis in 0..2 {
            assert!(
                (viewport[axis] - at[axis] - size[axis] - f64::from(card.offset[axis])).abs()
                    < 0.001
            );
        }
    }
}
