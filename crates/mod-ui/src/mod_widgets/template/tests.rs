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
