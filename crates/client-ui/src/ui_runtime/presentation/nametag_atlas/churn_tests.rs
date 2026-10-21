use super::super::nametags::{build_nametag_scene, tests::anchor};
use super::eviction_tests::seed_cell;
use super::*;

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
