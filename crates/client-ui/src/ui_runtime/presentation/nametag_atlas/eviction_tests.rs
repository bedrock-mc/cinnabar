use super::super::nametags::NAMETAG_RECORDS_PER_LINE;
use super::*;
use std::cell::Cell;

/// Seeds valid cells without glyph work so tests can reach count and pixel limits cheaply.
pub(super) fn seed_cell(atlas: &mut NametagAtlas, text: Arc<str>, width: u32, height: u32) {
    let (shelf, [x, y]) = atlas.allocate(width, height).unwrap();
    let line = AtlasLine {
        cell: [x, y, width, height],
        width_px: width as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
        top_px: 0.0,
        ink_y_px: [0.0, height as f32 / FONT_DESIGN_PIXEL_TEXELS as f32],
    };
    atlas.lines.insert(Arc::clone(&text), atlas.cells.len());
    atlas.cells.push(AtlasCell {
        text: Some(text),
        line,
        allocation: [x, y, width, height],
        rgba8: vec![255; (width * height * 4) as usize].into(),
        shelf,
        animation: None,
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
        assert_eq!(Some(text), atlas.cells[index].text.as_ref());
    }
}

#[test]
fn rejected_ink_never_samples_pixels_and_does_not_block_a_smaller_miss() {
    let mut atlas = full_atlas();
    let wide: Arc<str> = "0".into();
    let small = Arc::from("A");
    let requested = [
        Arc::from("B"),
        Arc::from("C"),
        Arc::clone(&wide),
        Arc::clone(&small),
    ];
    atlas.begin_frame(requested.iter());
    let font = shelf_font();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    let measured = measure_line(&wide, &font, &mut layouts).unwrap();
    assert_eq!(
        measured.size,
        [
            NAMETAG_ATLAS_SIDE - CELL_PADDING_TEXELS,
            SHORT_SHELF_HEIGHT - CELL_PADDING_TEXELS
        ]
    );
    // The previous order rasterized this ink before discovering it could not be placed.
    let _ = rasterize_line(
        &measured,
        &pages,
        &ui::FormattingPalette::default(),
        ui::TextEffects::default(),
    );
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
    let before = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &|page| {
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
        None,
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
    assert_eq!(
        after.records.len(),
        before.records.len() + NAMETAG_RECORDS_PER_LINE
    );
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
    let lines = render_model::MAX_NAMETAG_RECORDS / NAMETAG_RECORDS_PER_LINE;
    anchors[0].lines = vec![Arc::from("A"); lines].into();
    anchors[0].view_depth = 2.0;
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
        None,
        &font,
        &mut TextLayoutCache::new(1, 1 << 20),
        &mut atlas,
        &|page| {
            reads.set(reads.get() + 1);
            font_page(&font, page)
        },
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(scene.records.len(), lines * NAMETAG_RECORDS_PER_LINE);
    assert!(
        scene
            .records
            .iter()
            .all(|record| record.anchor == anchors[0].position.to_array())
    );
}

const SHORT_SHELF_HEIGHT: u32 = NAMETAG_ATLAS_SIDE / 4;

/// Supplies full-width ink at one, two and three short-shelf heights.
fn shelf_font() -> RuntimeFontCatalog {
    let font = ui_engine::test_support::fixture_font();
    let glyphs = [
        ('0', SHORT_SHELF_HEIGHT),
        ('2', SHORT_SHELF_HEIGHT * 2),
        ('/', SHORT_SHELF_HEIGHT * 3),
    ]
    .map(|(codepoint, height)| assets::SheetGlyph {
        metrics: *font.glyph(codepoint).unwrap(),
        draw_size_64: [
            (NAMETAG_ATLAS_SIDE - CELL_PADDING_TEXELS) * 64,
            (height - CELL_PADDING_TEXELS) * 64,
        ],
    });
    font.with_glyphs(&glyphs, |_| true)
}

#[test]
fn count_pressure_reuses_empty_and_unused_shelves_for_tall_ink() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let mut atlas = NametagAtlas::default();
    let shelf_count = (NAMETAG_ATLAS_SIDE / SHORT_SHELF_HEIGHT) as usize;
    let lines_per_shelf = MAX_ATLAS_LINES / shelf_count;
    let mean_width = NAMETAG_ATLAS_SIDE / lines_per_shelf as u32;
    assert_eq!(lines_per_shelf % 2, 0);
    assert_eq!(mean_width * lines_per_shelf as u32, NAMETAG_ATLAS_SIDE);
    for index in 0..MAX_ATLAS_LINES {
        let width = if index % 2 == 0 {
            mean_width - 1
        } else {
            mean_width + 1
        };
        seed_cell(
            &mut atlas,
            format!("cached {index}").into(),
            width,
            SHORT_SHELF_HEIGHT,
        );
    }
    assert_eq!(atlas.shelves.len(), shelf_count);
    let before = atlas.publish().0;
    let live_indices = [
        (shelf_count - 2) * lines_per_shelf,
        (shelf_count - 1) * lines_per_shelf,
    ];
    let requested = [
        Arc::from("2"),
        Arc::from(format!("cached {}", live_indices[0])),
        Arc::from(format!("cached {}", live_indices[1])),
    ];
    let font = shelf_font();
    let mut layouts = TextLayoutCache::new(requested.len(), 1 << 20);
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    atlas.begin_frame(requested.iter());
    // Admission itself must reclaim a line slot, then combine the emptied shelf with its neighbor.
    let tall = atlas
        .line(&requested[0], &font, &mut layouts, &pages)
        .unwrap();
    assert_eq!(
        tall.cell,
        [
            1,
            1,
            NAMETAG_ATLAS_SIDE - CELL_PADDING_TEXELS,
            SHORT_SHELF_HEIGHT * 2 - CELL_PADDING_TEXELS
        ]
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(atlas.shelves.len(), shelf_count);
    for (&old_index, text) in live_indices.iter().zip(&requested[1..]) {
        assert_eq!(
            atlas.line(text, &font, &mut layouts, &pages).unwrap().cell,
            before[old_index].cell
        );
        let cell = &atlas.cells[atlas.lines[text]];
        assert!(Arc::ptr_eq(&cell.rgba8, &before[old_index].rgba8));
    }
    let anchors: Vec<_> = requested
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let mut tag = anchor(text);
            tag.runtime_id = index as u64 + 1;
            tag.position.x = index as f32;
            tag.depth_tested = index == 1;
            tag
        })
        .collect();
    let scene = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    let built = layouts.built_layout_count();
    for _ in 0..3 {
        let repeated = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
        assert_eq!(repeated.records, scene.records);
        assert!(Arc::ptr_eq(&repeated.atlas, &scene.atlas));
        assert_eq!(repeated.atlas_revision, scene.atlas_revision);
        assert_eq!(layouts.built_layout_count(), built);
        assert_eq!(reads.get(), 1);
        assert_eq!(atlas.shelves.len(), shelf_count);
    }
}

#[test]
fn empty_spatial_shelves_never_satisfy_count_reclamation_or_republish_pixels() {
    let mut atlas = NametagAtlas::default();
    let font = shelf_font();
    let shelf_count = (NAMETAG_ATLAS_SIDE / SHORT_SHELF_HEIGHT) as usize;
    let mut layouts = TextLayoutCache::new(shelf_count + 1, 1 << 20);
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    for length in 1..=shelf_count {
        // Distinct cache keys retain a single full-width glyph without wrapping.
        let text = Arc::from(format!("0{}", "§r".repeat(length)));
        let placed = atlas.line(&text, &font, &mut layouts, &pages).unwrap();
        assert_eq!(
            placed.cell[2..],
            [
                NAMETAG_ATLAS_SIDE - CELL_PADDING_TEXELS,
                SHORT_SHELF_HEIGHT - CELL_PADDING_TEXELS
            ]
        );
    }
    assert_eq!(atlas.shelves.len(), shelf_count);
    let tall = Arc::from("/");
    atlas.begin_frame(std::iter::once(&tall));
    for remaining in (0..shelf_count).rev() {
        assert!(atlas.reclaim_shelf(None));
        assert_eq!(atlas.lines.len(), remaining);
    }
    let empty = atlas.publish();
    assert!(empty.0.is_empty());
    for _ in 0..3 {
        assert!(!atlas.reclaim_shelf(None));
        let unchanged = atlas.publish();
        assert!(Arc::ptr_eq(&unchanged.0, &empty.0));
        assert_eq!(unchanged.1, empty.1);
    }
    for _ in 0..2 {
        assert!(atlas.reclaim_shelf(Some([NAMETAG_ATLAS_SIDE, SHORT_SHELF_HEIGHT * 2])));
        assert!(!atlas.reclaim_shelf(None));
        let unchanged = atlas.publish();
        assert!(Arc::ptr_eq(&unchanged.0, &empty.0));
        assert_eq!(unchanged.1, empty.1);
    }
    reads.set(0);
    // This admission must cross the zero-height slot left by the earlier empty-shelf merge.
    let placed = atlas.line(&tall, &font, &mut layouts, &pages).unwrap();
    assert_eq!(
        placed.cell,
        [
            1,
            1,
            NAMETAG_ATLAS_SIDE - CELL_PADDING_TEXELS,
            SHORT_SHELF_HEIGHT * 3 - CELL_PADDING_TEXELS
        ]
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(atlas.shelves.len(), shelf_count);
    let published = atlas.publish();
    atlas.begin_frame(std::iter::once(&tall));
    assert_eq!(atlas.line(&tall, &font, &mut layouts, &pages), Some(placed));
    assert!(!atlas.reclaim_shelf(None));
    let unchanged = atlas.publish();
    assert!(Arc::ptr_eq(&unchanged.0, &published.0));
    assert_eq!(unchanged.1, published.1);
    assert_eq!(reads.get(), 1);
}

const SHORT_LINE_SIZE: [u32; 2] = [NAMETAG_ATLAS_SIDE / 16, TEXT_LINE_HEIGHT_64 / 64 * 2];

/// Supplies short lines and both narrow and full-width atlas-height glyphs.
fn changing_height_font() -> RuntimeFontCatalog {
    let font = ui_engine::test_support::fixture_font();
    let glyphs = [
        ('0', SHORT_LINE_SIZE),
        ('2', [1, NAMETAG_ATLAS_SIDE]),
        ('/', [NAMETAG_ATLAS_SIDE; 2]),
    ]
    .map(|(codepoint, size)| assets::SheetGlyph {
        metrics: *font.glyph(codepoint).unwrap(),
        draw_size_64: size.map(|value| value.saturating_sub(CELL_PADDING_TEXELS).max(1) * 64),
    });
    font.with_glyphs(&glyphs, |_| true)
}

/// Builds distinct cache keys for a crowd of single-glyph short labels.
fn short_anchors() -> Vec<super::super::nametags::NametagAnchor> {
    use super::super::nametags::{MAX_PRESENTED_NAMETAGS, tests::anchor};
    (1..=MAX_PRESENTED_NAMETAGS)
        .map(|index| {
            let mut tag = anchor(&format!("0{}", "§r".repeat(index)));
            tag.runtime_id = index as u64;
            tag.position.x = index as f32;
            tag
        })
        .collect()
}

/// Checks cell ownership and coverage after shelf indices have changed.
fn assert_placed_cells(atlas: &NametagAtlas) {
    assert_eq!(atlas.lines.len(), atlas.cells.len());
    assert!(atlas.cells.len() <= MAX_ATLAS_LINES);
    for (index, shelf) in atlas.shelves.iter().enumerate() {
        let end = shelf.y + shelf.height;
        assert!(end <= NAMETAG_ATLAS_SIDE);
        if let Some(next) = atlas.shelves.get(index + 1) {
            assert!(end <= next.y);
        }
    }
    for (index, cell) in atlas.cells.iter().enumerate() {
        assert_eq!(atlas.lines[cell.text.as_ref().unwrap()], index);
        let shelf = &atlas.shelves[cell.shelf];
        let [x, y, width, height] = cell.allocation;
        assert_eq!(cell.rgba8.len(), (width * height * 4) as usize);
        assert_eq!(y, shelf.y);
        assert!(height <= shelf.height);
        assert!(x + width <= NAMETAG_ATLAS_SIDE && y + height <= NAMETAG_ATLAS_SIDE);
        for other in &atlas.cells[..index] {
            let [ox, oy, ow, oh] = other.allocation;
            assert!(x + width <= ox || ox + ow <= x || y + height <= oy || oy + oh <= y);
        }
    }
}

#[test]
fn tall_to_short_and_back_admits_the_whole_crowd_without_waiting_for_a_change() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let font = changing_height_font();
    let anchors = short_anchors();
    let pages = |page| font_page(&font, page);
    let expected = build_nametag_scene(
        &anchors,
        None,
        &font,
        &mut TextLayoutCache::new(anchors.len(), 1 << 20),
        &mut NametagAtlas::default(),
        &pages,
    );
    assert_eq!(expected.atlas.len(), anchors.len());
    assert!(
        expected
            .atlas
            .iter()
            .all(|cell| cell.cell[2..] == SHORT_LINE_SIZE)
    );
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(anchors.len() + 1, 1 << 20);
    let reads = Cell::new(0);
    let counted_pages = |page| {
        reads.set(reads.get() + 1);
        pages(page)
    };
    let columns = NAMETAG_ATLAS_SIDE as usize / SHORT_LINE_SIZE[0] as usize;
    let shelf_bound = anchors.len().div_ceil(columns) + 1;
    for _ in 0..3 {
        let tall = build_nametag_scene(
            &[anchor("/")],
            None,
            &font,
            &mut layouts,
            &mut atlas,
            &counted_pages,
        );
        assert_eq!(tall.atlas.len(), 1);
        assert_eq!(
            tall.atlas[0].cell,
            [0, 0, NAMETAG_ATLAS_SIDE, NAMETAG_ATLAS_SIDE]
        );
        reads.set(0);
        let short = build_nametag_scene(
            &anchors,
            None,
            &font,
            &mut layouts,
            &mut atlas,
            &counted_pages,
        );
        assert_eq!(short.records, expected.records);
        assert_eq!(short.atlas.as_ref(), expected.atlas.as_ref());
        assert_eq!(reads.get(), anchors.len());
        assert!(atlas.shelves.len() <= shelf_bound);
        assert_placed_cells(&atlas);
        let built = layouts.built_layout_count();
        for _ in 0..3 {
            let repeated = build_nametag_scene(
                &anchors,
                None,
                &font,
                &mut layouts,
                &mut atlas,
                &counted_pages,
            );
            assert_eq!(repeated.records, short.records);
            assert!(Arc::ptr_eq(&repeated.atlas, &short.atlas));
            assert_eq!(repeated.atlas_revision, short.atlas_revision);
            assert_eq!(reads.get(), anchors.len());
            assert_eq!(layouts.built_layout_count(), built);
        }
    }
}

#[test]
fn reclaiming_a_tall_cell_releases_its_tail_without_moving_short_requested_cells() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let font = changing_height_font();
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(256, 1 << 20);
    let pages = |page| font_page(&font, page);
    atlas
        .line(&Arc::from("2"), &font, &mut layouts, &pages)
        .unwrap();
    let kept = atlas
        .line(&Arc::from("0"), &font, &mut layouts, &pages)
        .unwrap();
    let mut protected = anchor("0");
    protected.runtime_id = 0;
    let before = build_nametag_scene(
        std::slice::from_ref(&protected),
        None,
        &font,
        &mut layouts,
        &mut atlas,
        &pages,
    );
    let old_pixels = Arc::clone(&atlas.cells[atlas.lines["0"]].rgba8);
    let mut anchors = short_anchors();
    anchors.insert(0, protected);
    let after = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    assert_eq!(
        after.records.len(),
        anchors.len() * NAMETAG_RECORDS_PER_LINE
    );
    assert_eq!(
        atlas.line(&Arc::from("0"), &font, &mut layouts, &pages),
        Some(kept)
    );
    assert!(Arc::ptr_eq(
        &old_pixels,
        &atlas.cells[atlas.lines["0"]].rgba8
    ));
    let protected: Vec<_> = after
        .records
        .iter()
        .copied()
        .filter(|record| record.anchor[0] == 0.0)
        .collect();
    assert_eq!(protected, before.records);
    assert_placed_cells(&atlas);
    let repeated = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    assert_eq!(repeated.records, after.records);
    assert!(Arc::ptr_eq(&repeated.atlas, &after.atlas));
}

#[test]
fn short_first_tall_ink_grows_through_both_unallocated_and_reclaimed_tails() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let font = changing_height_font();
    let mut short = anchor("0");
    short.runtime_id = 1;
    let mut tall = anchor("2");
    tall.runtime_id = 2;
    tall.position.x = 2.0;
    let anchors = [short, tall];
    let pages = |page| font_page(&font, page);
    let expected = build_nametag_scene(
        &anchors,
        None,
        &font,
        &mut TextLayoutCache::new(3, 1 << 20),
        &mut NametagAtlas::default(),
        &pages,
    );
    assert_eq!(
        expected.records.len(),
        anchors.len() * NAMETAG_RECORDS_PER_LINE
    );
    for reclaimed_tail in [false, true] {
        let mut atlas = NametagAtlas::default();
        let mut layouts = TextLayoutCache::new(3, 1 << 20);
        if reclaimed_tail {
            build_nametag_scene(
                &[anchor("/")],
                None,
                &font,
                &mut layouts,
                &mut atlas,
                &pages,
            );
        }
        let before =
            build_nametag_scene(&anchors[..1], None, &font, &mut layouts, &mut atlas, &pages);
        let kept = atlas.cells[atlas.lines["0"]].line;
        let pixels = Arc::clone(&atlas.cells[atlas.lines["0"]].rgba8);
        let reads = Cell::new(0);
        let counted_pages = |page| {
            reads.set(reads.get() + 1);
            pages(page)
        };
        let after = build_nametag_scene(
            &anchors,
            None,
            &font,
            &mut layouts,
            &mut atlas,
            &counted_pages,
        );
        assert_eq!(after.records, expected.records);
        assert_eq!(after.atlas.as_ref(), expected.atlas.as_ref());
        assert_eq!(atlas.cells[atlas.lines["0"]].line, kept);
        assert!(Arc::ptr_eq(&atlas.cells[atlas.lines["0"]].rgba8, &pixels));
        assert_eq!(&after.records[..before.records.len()], &before.records);
        assert_eq!(reads.get(), 1);
        assert_placed_cells(&atlas);
        for _ in 0..3 {
            let repeated = build_nametag_scene(
                &anchors,
                None,
                &font,
                &mut layouts,
                &mut atlas,
                &counted_pages,
            );
            assert_eq!(repeated.records, after.records);
            assert!(Arc::ptr_eq(&repeated.atlas, &after.atlas));
            assert_eq!(repeated.atlas_revision, after.atlas_revision);
            assert_eq!(reads.get(), 1);
        }
    }
}

/// Checks that every sampled cell has current pixels after an extraction skips publications.
fn assert_scene_updates(scene: &render_model::NametagScene, previous: &[NametagAtlasRect]) {
    let mut pixels = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
    super::tests::apply_rectangles(&mut pixels, previous, &[]);
    super::tests::apply_rectangles(&mut pixels, &scene.atlas, previous);
    for record in scene.records.iter().filter(|record| record.text != 0) {
        let [left, top, right, bottom] =
            record.uv.map(|uv| (uv * NAMETAG_ATLAS_SIDE as f32) as u32);
        let cell = [left, top, right - left, bottom - top];
        let rectangle = scene
            .atlas
            .iter()
            .find(|rectangle| {
                let [x, y, width, height] = rectangle.cell;
                x <= left && y <= top && x + width >= right && y + height >= bottom
            })
            .unwrap();
        for row in 0..cell[3] as usize {
            let start = ((top as usize + row) * NAMETAG_ATLAS_SIDE as usize + left as usize) * 4;
            let source = (((top - rectangle.cell[1]) as usize + row) * rectangle.cell[2] as usize
                + (left - rectangle.cell[0]) as usize)
                * 4;
            assert_eq!(
                &pixels[start..start + cell[2] as usize * 4],
                &rectangle.rgba8[source..source + cell[2] as usize * 4],
            );
        }
    }
}

#[test]
fn partially_live_shelves_recover_before_publication_and_match_a_fresh_scene() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let base = ui_engine::test_support::fixture_font();
    let height = TEXT_LINE_HEIGHT_64.div_ceil(64);
    let live_width = 100;
    let wide_width = 2000;
    let glyphs = [
        ('0', live_width),
        (
            '2',
            NAMETAG_ATLAS_SIDE - live_width - 2 * CELL_PADDING_TEXELS,
        ),
        ('/', wide_width),
    ]
    .map(|(codepoint, width)| assets::SheetGlyph {
        metrics: *base.glyph(codepoint).unwrap(),
        draw_size_64: [width * 64, height * 64],
    });
    let font = base.with_glyphs(&glyphs, |_| true);
    let rows = (NAMETAG_ATLAS_SIDE / (height + CELL_PADDING_TEXELS)) as usize;
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(rows * 2 + 2, 1 << 20);
    let pages = |page| font_page(&font, page);
    let mut anchors = Vec::new();
    for row in 0..rows {
        let live = format!("0{}", "§r".repeat(row));
        for text in [live.clone(), format!("2{}", "§r".repeat(row))] {
            atlas
                .line(&text.into(), &font, &mut layouts, &pages)
                .unwrap();
        }
        let mut tag = anchor(&live);
        tag.runtime_id = row as u64;
        tag.position.x = row as f32;
        tag.depth_tested = row % 2 == 0;
        anchors.push(tag);
    }
    assert_eq!(atlas.shelves.len(), rows);
    let original = atlas.publish();
    for (index, text) in ["/".to_owned(), format!("0{}", "§r".repeat(rows))]
        .iter()
        .enumerate()
    {
        let mut tag = anchor(text);
        tag.runtime_id = (rows + index) as u64;
        tag.position.x = (rows + index) as f32;
        anchors.push(tag);
    }
    let reads = Cell::new(0);
    let counted_pages = |page| {
        reads.set(reads.get() + 1);
        pages(page)
    };
    let blocked = build_nametag_scene(
        &anchors,
        None,
        &font,
        &mut layouts,
        &mut atlas,
        &counted_pages,
    );
    assert!(atlas.lines.contains_key("/"));
    assert_eq!(
        blocked.records.len(),
        anchors.len() * NAMETAG_RECORDS_PER_LINE
    );
    // The smaller preflight miss precedes one fresh packing attempt for the whole live set.
    assert_eq!(reads.get(), 1 + anchors.len());
    let blocked_copy = blocked.clone();
    let recovered = build_nametag_scene(
        &anchors,
        None,
        &font,
        &mut layouts,
        &mut atlas,
        &counted_pages,
    );
    let expected = build_nametag_scene(
        &anchors,
        None,
        &font,
        &mut TextLayoutCache::new(anchors.len(), 1 << 20),
        &mut NametagAtlas::default(),
        &pages,
    );
    assert_eq!(recovered.records, expected.records);
    assert_eq!(recovered.atlas.as_ref(), expected.atlas.as_ref());
    assert_eq!(recovered.atlas.len(), anchors.len());
    assert_eq!(reads.get(), 1 + anchors.len());
    assert_eq!(recovered.atlas_revision, blocked.atlas_revision);
    assert_ne!(recovered.atlas[1].cell, original.0[2].cell);
    assert_eq!(blocked, blocked_copy);
    assert_scene_updates(&blocked, &original.0);
    assert_scene_updates(&recovered, &original.0);
    assert_scene_updates(&recovered, &blocked.atlas);
    assert_placed_cells(&atlas);
    let built = layouts.built_layout_count();
    for _ in 0..4 {
        let repeated = build_nametag_scene(
            &anchors,
            None,
            &font,
            &mut layouts,
            &mut atlas,
            &counted_pages,
        );
        assert_eq!(repeated.records, recovered.records);
        assert!(Arc::ptr_eq(&repeated.atlas, &recovered.atlas));
        assert_eq!(repeated.atlas_revision, recovered.atlas_revision);
        assert_eq!(reads.get(), 1 + anchors.len());
        assert_eq!(layouts.built_layout_count(), built);
    }
}

#[test]
fn impossible_requests_rebuild_once_until_actual_admission_order_changes() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let font = changing_height_font();
    let mut full = anchor("/");
    full.runtime_id = 1;
    let mut short = anchor("0");
    short.runtime_id = 2;
    short.position.x = 2.0;
    let mut anchors = [full, short];
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(2, 1 << 20);
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    let initial = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    assert_eq!(initial.records.len(), NAMETAG_RECORDS_PER_LINE);
    assert_eq!(reads.get(), 2);
    let rebuilt = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    assert_eq!(rebuilt.records, initial.records);
    assert_eq!(reads.get(), 2);
    assert_eq!(rebuilt.atlas_revision, initial.atlas_revision);
    for _ in 0..4 {
        let repeated = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
        assert_eq!(repeated, rebuilt);
        assert!(Arc::ptr_eq(&repeated.atlas, &rebuilt.atlas));
        assert_eq!(reads.get(), 2);
    }
    // The same requested set in a new order must get its own fresh admission attempt.
    anchors[1].view_depth -= 1.0;
    build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    let reordered = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    let expected = build_nametag_scene(
        &anchors,
        None,
        &font,
        &mut TextLayoutCache::new(2, 1 << 20),
        &mut NametagAtlas::default(),
        &|page| font_page(&font, page),
    );
    assert_eq!(reordered.records, expected.records);
    assert_eq!(reordered.atlas.as_ref(), expected.atlas.as_ref());
    assert_eq!(reads.get(), 3);
    for _ in 0..4 {
        let repeated = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
        assert_eq!(repeated, reordered);
        assert!(Arc::ptr_eq(&repeated.atlas, &reordered.atlas));
        assert_eq!(reads.get(), 3);
    }
    assert_placed_cells(&atlas);
}

#[test]
fn rebuild_retry_includes_lines_reached_after_rejected_budget_prefixes() {
    use super::super::nametags::{build_nametag_scene, tests::anchor};
    let font = changing_height_font();
    let mut prefix = anchor("0");
    prefix.runtime_id = 1;
    let prefix_lines = render_model::MAX_NAMETAG_RECORDS / NAMETAG_RECORDS_PER_LINE;
    prefix.lines = std::iter::once(Arc::from("0"))
        .chain(std::iter::repeat_n(Arc::from("/"), prefix_lines - 1))
        .collect::<Vec<_>>()
        .into();
    let mut late = anchor("/§r");
    late.runtime_id = 2;
    late.position.x = 2.0;
    let mut anchors = [prefix, late];
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(4, 1 << 20);
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    let rebuilt = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    assert_eq!(reads.get(), 2);
    for _ in 0..3 {
        let repeated = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
        assert_eq!(repeated, rebuilt);
        assert!(Arc::ptr_eq(&repeated.atlas, &rebuilt.atlas));
        assert_eq!(reads.get(), 2);
    }
    // The predicted budget prefix stays identical; only a later actual request changes.
    anchors[1].lines = vec![Arc::from("2")].into();
    let changed = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    assert_eq!(
        changed.records.len(),
        anchors.len() * NAMETAG_RECORDS_PER_LINE
    );
    assert_eq!(reads.get(), 3);
    let recovered = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
    let expected = build_nametag_scene(
        &anchors,
        None,
        &font,
        &mut TextLayoutCache::new(4, 1 << 20),
        &mut NametagAtlas::default(),
        &|page| font_page(&font, page),
    );
    assert_eq!(recovered.records, expected.records);
    assert_eq!(recovered.atlas.as_ref(), expected.atlas.as_ref());
    assert_eq!(reads.get(), 5);
    for _ in 0..3 {
        let repeated = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &pages);
        assert_eq!(repeated, recovered);
        assert!(Arc::ptr_eq(&repeated.atlas, &recovered.atlas));
        assert_eq!(reads.get(), 5);
    }
    assert_placed_cells(&atlas);
}
