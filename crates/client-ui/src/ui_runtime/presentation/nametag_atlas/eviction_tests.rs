use super::*;
use std::cell::Cell;

/// Seeds valid cells without glyph work so tests can reach count and pixel limits cheaply.
fn seed_cell(atlas: &mut NametagAtlas, text: Arc<str>, width: u32, height: u32) {
    let (shelf, [x, y]) = atlas.allocate(width, height).unwrap();
    let line = AtlasLine {
        cell: [x, y, width, height],
        width_px: width as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
        top_px: 0.0,
    };
    atlas.lines.insert(Arc::clone(&text), atlas.cells.len());
    atlas.cells.push(AtlasCell {
        text,
        line,
        rgba8: vec![255; (width * height * 4) as usize].into(),
        shelf,
    });
    atlas.published = None;
}

/// Fills two shelves with one active and one unused cell each.
fn full_atlas() -> NametagAtlas {
    let mut atlas = NametagAtlas::default();
    for text in ["B", "unused 0", "C", "unused 1"] {
        seed_cell(
            &mut atlas,
            text.into(),
            NAMETAG_ATLAS_SIDE / 2,
            NAMETAG_ATLAS_SIDE / 2,
        );
    }
    atlas
}

#[test]
fn count_limit_hits_and_repeated_requests_do_not_republish_or_rasterize() {
    let mut atlas = NametagAtlas::default();
    for index in 0..MAX_ATLAS_LINES {
        seed_cell(&mut atlas, format!("cached {index}").into(), 1, 1);
    }
    let before = atlas.publish();
    let text = Arc::from("cached 0");
    atlas.begin_frame(std::iter::repeat_n(&text, MAX_ATLAS_LINES));
    assert!(atlas.has_room_for(0));
    assert!(!atlas.has_room_for(1));
    let font = ui_engine::test_support::fixture_font();
    let reads = Cell::new(0);
    let mut layouts = TextLayoutCache::new(1, 1 << 20);
    for _ in 0..100 {
        assert!(
            atlas
                .line(&text, &font, &mut layouts, &|page| {
                    reads.set(reads.get() + 1);
                    font_page(&font, page)
                })
                .is_some()
        );
    }
    assert_eq!(reads.get(), 0);
    assert_eq!(layouts.built_layout_count(), 0);
    let after = atlas.publish();
    assert!(Arc::ptr_eq(&before.0, &after.0));
    assert_eq!(before.1, after.1);
}

#[test]
fn a_miss_reclaims_one_shelf_and_keeps_later_hits_and_other_unused_pixels() {
    let mut atlas = full_atlas();
    let before = atlas.publish().0;
    let requested = [Arc::from("A"), Arc::from("B"), Arc::from("C")];
    atlas.begin_frame(requested.iter());
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let reads = Cell::new(0);
    for text in requested.iter().chain(std::iter::once(&requested[0])) {
        atlas
            .line(text, &font, &mut layouts, &|page| {
                reads.set(reads.get() + 1);
                font_page(&font, page)
            })
            .unwrap();
    }
    assert_eq!(
        reads.get(),
        1,
        "only the new single-glyph line samples pixels"
    );
    assert!(!atlas.lines.contains_key("unused 0"));
    assert!(atlas.lines.contains_key("unused 1"));
    let after = atlas.publish().0;
    for old in [&before[0], &before[2], &before[3]] {
        assert!(
            after
                .iter()
                .any(|new| new.cell == old.cell && Arc::ptr_eq(&new.rgba8, &old.rgba8))
        );
    }
    assert_eq!(NametagAtlasRect::updates(&after, &before).count(), 2);
    for (text, &index) in &atlas.lines {
        assert_eq!(text, &atlas.cells[index].text);
    }
}

#[test]
fn rejected_ink_never_samples_pixels_and_does_not_block_a_smaller_miss() {
    let mut atlas = full_atlas();
    let wide: Arc<str> = "A".repeat(NAMETAG_ATLAS_SIDE as usize).into();
    let small = Arc::from("A");
    let requested = [
        Arc::from("B"),
        Arc::from("C"),
        Arc::clone(&wide),
        Arc::clone(&small),
    ];
    atlas.begin_frame(requested.iter());
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    let measured = measure_line(&wide, &font, &mut layouts).unwrap();
    assert_eq!(measured.size[0], NAMETAG_ATLAS_SIDE);
    // The previous order rasterized this ink before discovering it could not be placed.
    let _ = rasterize_line(&measured, &pages, &ui::FormattingPalette::default());
    assert!(reads.get() > 0);
    reads.set(0);
    assert!(atlas.line(&wide, &font, &mut layouts, &pages).is_none());
    assert_eq!(reads.get(), 0);
    assert!(atlas.line(&small, &font, &mut layouts, &pages).is_some());
    assert_eq!(reads.get(), 1);
}

#[test]
fn adjoining_unused_shelves_fit_tall_ink_without_moving_a_live_shelf() {
    let mut atlas = NametagAtlas::default();
    let quarter = NAMETAG_ATLAS_SIDE / 4;
    for text in ["live", "old 1", "old 2", "old 3"] {
        seed_cell(&mut atlas, text.into(), NAMETAG_ATLAS_SIDE, quarter);
    }
    let before = atlas.publish().0;
    let requested = [Arc::from("live"), Arc::from("tall")];
    atlas.begin_frame(requested.iter());
    assert!(atlas.reclaim_shelf(Some([NAMETAG_ATLAS_SIDE, quarter * 2])));
    let (_, origin) = atlas.allocate(NAMETAG_ATLAS_SIDE, quarter * 2).unwrap();
    assert_eq!(origin, [0, quarter]);
    assert!(atlas.lines.contains_key("old 3"));
    let after = atlas.publish().0;
    assert_eq!(after[0].cell, before[0].cell);
    assert!(Arc::ptr_eq(&after[0].rgba8, &before[0].rgba8));
}

#[test]
fn skipped_publications_and_old_readers_keep_reused_cell_pixels() {
    let mut atlas = full_atlas();
    let first = atlas.publish().0;
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    for name in ["A", "AA"] {
        let requested = [Arc::from(name), Arc::from("B"), Arc::from("C")];
        atlas.begin_frame(requested.iter());
        atlas
            .line(&requested[0], &font, &mut layouts, &|page| {
                font_page(&font, page)
            })
            .unwrap();
        let _ = atlas.publish();
    }
    let latest = atlas.publish().0;
    let mut incremental = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
    super::tests::apply_rectangles(&mut incremental, &first, &[]);
    super::tests::apply_rectangles(&mut incremental, &latest, &first);
    for rectangle in latest.iter() {
        let [x, y, width, height] = rectangle.cell;
        for row in 0..height as usize {
            let start = ((y as usize + row) * NAMETAG_ATLAS_SIDE as usize + x as usize) * 4;
            let source = row * width as usize * 4;
            assert_eq!(
                &incremental[start..start + width as usize * 4],
                &rectangle.rgba8[source..source + width as usize * 4]
            );
        }
    }
    assert_eq!(first.len(), 4);
    assert!(
        first
            .iter()
            .all(|rectangle| rectangle.rgba8.iter().all(|&byte| byte == 255))
    );
}

#[test]
fn cache_pressure_preserves_complete_label_records_and_rasterizes_only_the_miss() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let mut atlas = NametagAtlas::default();
    let mut anchors = [anchor("B"), anchor("C")];
    anchors[1].runtime_id = 2;
    anchors[1].position.x = 2.0;
    anchors[1].depth_tested = true;
    let before = build_nametag_scene(&anchors, 1.0, &font, &mut layouts, &mut atlas, &|page| {
        font_page(&font, page)
    });
    for index in atlas.lines.len()..MAX_ATLAS_LINES {
        seed_cell(&mut atlas, format!("old {index}").into(), 1, 1);
    }
    let mut new = anchor("A");
    new.runtime_id = 3;
    new.position.x = 4.0;
    new.view_depth = 10.0;
    let reads = Cell::new(0);
    let after = build_nametag_scene(
        &[new, anchors[0].clone(), anchors[1].clone()],
        1.0,
        &font,
        &mut layouts,
        &mut atlas,
        &|page| {
            reads.set(reads.get() + 1);
            font_page(&font, page)
        },
    );
    assert_eq!(reads.get(), 1);
    let retained: Vec<_> = after
        .records
        .iter()
        .copied()
        .filter(|record| record.anchor[0] != 4.0)
        .collect();
    assert_eq!(before.records, retained);
    assert_eq!(after.records.len(), before.records.len() + 3);
    for old in before.atlas.iter() {
        assert!(
            after
                .atlas
                .iter()
                .any(|new| old.cell == new.cell && Arc::ptr_eq(&old.rgba8, &new.rgba8))
        );
    }
}

#[test]
fn labels_outside_the_record_budget_do_not_block_admission() {
    use super::super::nametags::{MAX_PRESENTED_NAMETAGS, build_nametag_scene, tests::anchor};
    let mut atlas = NametagAtlas::default();
    let mut anchors = vec![anchor("A")];
    let lines = render_model::MAX_NAMETAG_RECORDS / 3;
    anchors[0].lines = vec![Arc::from("A"); lines].into();
    anchors[0].view_depth = 10.0;
    for first in (0..MAX_ATLAS_LINES).step_by(MAX_PRESENTED_NAMETAGS) {
        let mut later = anchor("");
        later.runtime_id = first as u64 + 2;
        later.lines = (first..(first + MAX_PRESENTED_NAMETAGS).min(MAX_ATLAS_LINES))
            .map(|index| {
                let text: Arc<str> = format!("cached {index}").into();
                seed_cell(&mut atlas, Arc::clone(&text), 1, 1);
                text
            })
            .collect();
        anchors.push(later);
    }
    assert_eq!(atlas.lines.len(), MAX_ATLAS_LINES);
    let font = ui_engine::test_support::fixture_font();
    let reads = Cell::new(0);
    let scene = build_nametag_scene(
        &anchors,
        1.0,
        &font,
        &mut TextLayoutCache::new(1, 1 << 20),
        &mut atlas,
        &|page| {
            reads.set(reads.get() + 1);
            font_page(&font, page)
        },
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(scene.records.len(), lines * 3);
    assert!(
        scene
            .records
            .iter()
            .all(|record| record.anchor == anchors[0].position.to_array())
    );
}
