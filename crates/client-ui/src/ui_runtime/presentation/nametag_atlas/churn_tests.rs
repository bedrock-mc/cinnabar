use super::super::nametags::{build_nametag_scene, tests::anchor};
use super::eviction_tests::seed_cell;
use super::*;
use std::cell::Cell;

#[test]
fn changing_one_line_in_a_full_atlas_rasterizes_only_that_line() {
    let font = tests::stem_font();
    let mut texts: Vec<Arc<str>> = (0..220)
        .map(|index| format!("{}{}", "A".repeat(320), "§r".repeat(index)).into())
        .collect();
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    let mut layouts = TextLayoutCache::new(512, 8 << 20);
    let mut atlas = NametagAtlas::default();
    atlas.prepare_frame(&texts, false, &font, &mut layouts, &pages);
    let mut before = atlas.publish().0;
    assert!(
        atlas.cells.len() < texts.len(),
        "fixture must exhaust the atlas"
    );
    for tick in 1..=3 {
        texts[0] = format!("{}{}", "B".repeat(320), "§r".repeat(tick)).into();
        reads.set(0);
        atlas.prepare_frame(&texts, false, &font, &mut layouts, &pages);
        let after = atlas.publish().0;
        assert_eq!(reads.get(), 320, "only the changed line samples its glyphs");
        assert_eq!(NametagAtlasRect::updates(&after, &before).count(), 1);
        before = after;
    }
}

#[test]
fn repacked_pixels_retire_when_the_font_or_palette_changes() {
    let font = tests::stem_font();
    let text = Arc::from("§cAA");
    let mut atlas = NametagAtlas::default();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let reads = Cell::new(0);
    let pages = |page| {
        reads.set(reads.get() + 1);
        font_page(&font, page)
    };
    atlas.line(&text, &font, &mut layouts, &pages).unwrap();
    let before = atlas.publish().0;
    atlas.repack();
    reads.set(0);
    atlas.line(&text, &font, &mut layouts, &pages).unwrap();
    assert_eq!(reads.get(), 0);
    assert!(Arc::ptr_eq(&before[0].rgba8, &atlas.publish().0[0].rgba8));

    atlas.repack();
    atlas.set_palette(ui::FormattingPalette::from_globals(|_| {
        Some([0.0, 1.0, 0.0])
    }));
    atlas.line(&text, &font, &mut layouts, &pages).unwrap();
    assert_eq!(reads.get(), 2);
    assert_ne!(before[0].rgba8, atlas.publish().0[0].rgba8);

    atlas.repack();
    atlas.reset();
    reads.set(0);
    atlas.line(&text, &font, &mut layouts, &pages).unwrap();
    assert_eq!(reads.get(), 2);
}

#[test]
fn fragmented_names_repack_before_publication_and_keep_every_changing_suffix() {
    let fixture = ui_engine::test_support::fixture_font();
    let mut wide = *fixture.glyph('0').unwrap();
    wide.codepoint = 'W';
    let font = fixture.with_glyphs(
        &[
            assets::SheetGlyph {
                metrics: *fixture.glyph('0').unwrap(),
                draw_size_64: [128 * 64, 36 * 64],
            },
            assets::SheetGlyph {
                metrics: wide,
                draw_size_64: [NAMETAG_ATLAS_SIDE * 64, 36 * 64],
            },
        ],
        |_| true,
    );
    let mut atlas = NametagAtlas::default();
    let mut anchors = Vec::new();
    for index in 0..32 {
        let mut tag = anchor(&format!("0{}", "§r".repeat(index)));
        tag.runtime_id = index as u64 + 1;
        tag.position.x = index as f32;
        seed_cell(&mut atlas, Arc::clone(&tag.lines[0]), 128, 36);
        seed_cell(
            &mut atlas,
            format!("unused {index}").into(),
            NAMETAG_ATLAS_SIDE - 128,
            64,
        );
        anchors.push(tag);
    }
    let mut wide = anchor("W");
    wide.runtime_id = 100;
    wide.position.x = 100.0;
    wide.view_depth = 2.0;
    anchors.push(wide);
    let mut layouts = TextLayoutCache::new(64, 1 << 20);
    for tick in 0..32 {
        anchors.last_mut().unwrap().lines =
            vec![Arc::from(format!("W{}", "§r".repeat(tick)))].into();
        let scene = build_nametag_scene(&anchors, None, &font, &mut layouts, &mut atlas, &|page| {
            font_page(&font, page)
        });
        assert_eq!(
            scene
                .records
                .iter()
                .filter(|record| record.text != 0)
                .count(),
            anchors.len(),
            "tick {tick}"
        );
        assert_eq!(scene.labels.len(), anchors.len());
        assert!(scene.records.iter().all(|record| record.line_lift == 0.0));
    }
}
