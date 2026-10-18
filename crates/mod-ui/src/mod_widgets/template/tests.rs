use super::*;

/// Center anchoring follows viewport and scale changes until a saved drag position overrides it.
#[test]
fn centered_compass_follows_viewport_and_saved_placement() {
    let mut card:Card=serde_json::from_str(r#"{"id":"direction","rows":[],"width":280,"offset":[0,8],"anchor":"top_center","compass":{}}"#).unwrap();
    assert_eq!(origin(&card, [800., 500.]), [260., 8.]);
    assert_eq!(origin(&card, [400., 200.]), [60., 8.]);
    card.scale = 2.;
    assert_eq!(origin(&card, [400., 200.]), [-80., 8.]);
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
