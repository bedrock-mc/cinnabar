use super::*;

#[test]
fn styling_uses_the_last_record_when_whole_labels_fit() {
    let tag = anchor(&"A\n".repeat(MAX_NAMETAG_RECORDS / NAMETAG_RECORDS_PER_LINE));
    let mut scene = scene(&[tag]);
    style_scene(
        &mut scene,
        &serde_json::from_value(serde_json::json!({})).unwrap(),
        None,
    );
    assert_eq!(scene.records.len(), MAX_NAMETAG_RECORDS);
}

#[test]
fn oversized_styled_labels_keep_the_nearest_renderable_prefix() {
    for shadow in [false, true] {
        for border in [false, true] {
            let style = serde_json::from_value(serde_json::json!({
                "shadow": shadow,
                "border_color": border.then_some([1.; 4])
            }))
            .unwrap();
            let mut near = anchor(&"A\n".repeat(MAX_NAMETAG_LINES));
            near.view_depth = 1.0;
            near.cinnabar_badge = true;
            near.set_counter_annotation(Some(Arc::new(ui::mod_hud::CounterAnnotation {
                prefix: Arc::from("Count: "),
                value: Arc::from("2"),
                prefix_color: [255; 4],
                value_color: [255; 4],
                separate: true,
                before: false,
            })));
            let mut far = anchor("B");
            far.runtime_id = 2;
            far.view_depth = 2.0;
            far.position.z = -10.0;
            let font = ui_engine::test_support::fixture_font();
            let mut painted = build_nametag_scene(
                &[far, near.clone()],
                Some(&style),
                &font,
                &mut TextLayoutCache::new(8, 1 << 20),
                &mut NametagAtlas::default(),
                &|page| super::super::super::nametag_atlas::font_page(&font, page),
            );
            style_scene(&mut painted, &style, None);
            assert!(
                !painted.records.is_empty(),
                "shadow {shadow}, border {border}"
            );
            assert!(painted.records.len() <= MAX_NAMETAG_RECORDS);
            assert!(
                painted
                    .records
                    .iter()
                    .all(|r| r.anchor == near.position.to_array())
            );
            let line_cost = 2 + usize::from(shadow) + 4 * usize::from(border);
            let extras = 3 * (1 + usize::from(shadow)) + 1 + 4 * usize::from(border);
            let lines = (MAX_NAMETAG_RECORDS - extras) / line_cost;
            assert_eq!(painted.records.len(), lines * line_cost + extras);
            assert!(
                painted
                    .records
                    .iter()
                    .all(|r| r.line_lift == EXTRA_LINE_LIFT * lines as f32)
            );
        }
    }
}

#[test]
fn counters_outside_the_record_budget_do_not_reserve_atlas_cells() {
    let mut near = anchor(&"A\n".repeat(MAX_NAMETAG_RECORDS / NAMETAG_RECORDS_PER_LINE));
    near.view_depth = 2.0;
    let mut far = anchor("B");
    far.runtime_id = 2;
    far.set_counter_annotation(
        ui::mod_hud::TotemCounterStyle::default()
            .annotation(2)
            .map(Arc::new),
    );
    let scene = scene(&[far, near]);
    assert_eq!(scene.records.len(), MAX_NAMETAG_RECORDS);
    assert_eq!(scene.atlas.len(), 1);
}

#[test]
fn styling_admits_complete_near_labels_and_keeps_back_to_front_order() {
    let anchors: Vec<_> = (1..=MAX_PRESENTED_NAMETAGS)
        .map(|index| {
            let mut tag = anchor("A\nB\nC\nD");
            tag.runtime_id = index as u64;
            tag.position.z = -(index as f32);
            tag.view_depth = index as f32;
            tag
        })
        .collect();
    let mut scene = scene(&anchors);
    let style = serde_json::from_value(serde_json::json!({
        "shadow": true, "border_color": [1., 1., 1., 1.]
    }))
    .unwrap();
    style_scene(&mut scene, &style, None);
    let records_per_label = 4 * 7;
    let labels = MAX_NAMETAG_RECORDS / records_per_label;
    assert_eq!(scene.records.len(), labels * records_per_label);
    for (index, records) in scene.records.chunks_exact(records_per_label).enumerate() {
        let expected = -((labels - index) as f32);
        assert!(records.iter().all(|record| record.anchor[2] == expected));
        assert_eq!(records.iter().filter(|record| record.text == 1).count(), 8);
    }
}

/// Supplies one name glyph that needs the entire padded atlas.
fn full_cell_font() -> RuntimeFontCatalog {
    let font = ui_engine::test_support::fixture_font();
    let mut metrics = *font.glyph('0').unwrap();
    metrics.codepoint = 'A';
    font.with_glyphs(
        &[assets::SheetGlyph {
            metrics,
            draw_size_64: [NAMETAG_ATLAS_SIDE * 64; 2],
        }],
        |_| true,
    )
}

#[test]
fn failed_name_rasters_keep_the_second_slot_badge_and_counter_positions() {
    let font = full_cell_font();
    let policy = ui::mod_hud::TotemCounterStyle {
        show_prefix: false,
        ..Default::default()
    };
    let mut tag = anchor("A\nB");
    tag.cinnabar_badge = true;
    tag.set_counter_annotation(policy.annotation(2).map(Arc::new));
    let scene = scene_with_font(&[tag], &font);
    let name = scene
        .records
        .iter()
        .find(|record| {
            record.text != 0 && record.color == [1.; 4] && record.rect[1] >= NAMETAG_LINE_PITCH_PX
        })
        .unwrap();
    assert_eq!(name.line_lift, EXTRA_LINE_LIFT);
    let counter = scene
        .records
        .iter()
        .find(|record| record.text != 0 && record.color[..3] != [1.; 3])
        .unwrap();
    assert!(counter.rect[1] < NAMETAG_LINE_PITCH_PX);
    assert_eq!(
        scene
            .records
            .iter()
            .filter(|record| record.text != 0)
            .count(),
        3
    );
}

#[test]
fn atlas_pressure_trims_failed_tail_slots_but_preserves_interior_gaps() {
    let font = full_cell_font();
    for (text, last_slot) in [
        (format!("B\n{}", "A\n".repeat(MAX_NAMETAG_LINES - 1)), 0),
        (String::from("B\nA\nB"), 2),
    ] {
        let painted = scene_with_font(&[anchor(&text)], &font);
        let text: Vec<_> = painted.records.iter().filter(|r| r.text != 0).collect();
        assert_eq!(text.len(), if last_slot == 0 { 1 } else { 2 });
        assert!(
            painted
                .records
                .iter()
                .all(|r| r.line_lift == EXTRA_LINE_LIFT * last_slot as f32)
        );
        assert_eq!(
            text.last().unwrap().rect[1] - text[0].rect[1],
            NAMETAG_LINE_PITCH_PX * last_slot as f32
        );
    }
}

#[test]
fn enabling_badges_retries_a_previously_exhausted_text_order() {
    let font = full_cell_font();
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let mut tag = anchor("A\nB");
    let pages = |page| super::super::super::nametag_atlas::font_page(&font, page);
    build_nametag_scene(
        &[tag.clone()],
        None,
        &font,
        &mut layouts,
        &mut atlas,
        &pages,
    );
    tag.cinnabar_badge = true;
    let scene = build_nametag_scene(&[tag], None, &font, &mut layouts, &mut atlas, &pages);
    assert_eq!(
        scene
            .records
            .iter()
            .filter(|record| record.text != 0)
            .count(),
        2
    );
    assert!(
        scene
            .records
            .iter()
            .any(|record| record.text != 0 && record.rect[1] >= NAMETAG_LINE_PITCH_PX)
    );
}
