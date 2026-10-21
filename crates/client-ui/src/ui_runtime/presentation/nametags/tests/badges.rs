use super::*;

/// Applying new roster publications updates both surfaces, including expired positive answers.
#[test]
fn badge_follows_roster_answers_expiry_and_display_setting() {
    let mut roster = crate::ui_runtime::player_roster::PlayerRoster::default();
    roster.set_entries(vec![("ExamplePlayer".into(), "11".into(), [0; 16])]);
    let mut tag = anchor("ExamplePlayer");
    for (reported, show) in [(true, true), (true, false), (true, true), (false, true)] {
        roster.set_show_badges(show);
        roster.set_cinnabar(Arc::new(if reported {
            HashSet::from(["11".to_owned()])
        } else {
            HashSet::new()
        }));
        tag.set_cinnabar_badge(roster.has_badge("11", &[0; 16]));
        assert_eq!(roster.rows()[0].badged, reported && show);
        assert_eq!(
            scene(std::slice::from_ref(&tag)).records.len(),
            if reported && show { 3 } else { 2 }
        );
    }
}

/// The gem occupies one glyph-height square plus the font's ordinary one-pixel gap.
#[test]
fn badge_widens_only_the_name_plate_and_centers_beside_glyphs() {
    let plain = scene(&[anchor("ExamplePlayer\n20")]);
    let mut tag = anchor("ExamplePlayer\n20");
    tag.set_cinnabar_badge(true);
    let badged = scene(&[tag]);
    let size = ui::FONT_INK_TEXELS as f32 / FONT_DESIGN_PIXEL_TEXELS as f32;
    let advance = size + 1.0;
    let old_width = plain.records[0].rect[2] - plain.records[0].rect[0];
    let new_width = badged.records[0].rect[2] - badged.records[0].rect[0];
    assert!((new_width - old_width - advance).abs() <= 1.0);
    assert_eq!(plain.records[1].rect, badged.records[1].rect);
    let text = &badged.records[2];
    let gem = &badged.records[4];
    assert_eq!(gem.rect[2] - gem.rect[0], size);
    assert_eq!((gem.rect[1] + gem.rect[3]) / 2.0, size / 2.0);
    assert_eq!(text.rect[0] - gem.rect[2], 1.0);
    assert_eq!(gem.rect[0], badged.records[0].rect[0] + 1.0);
    assert_eq!(badged.records.len(), plain.records.len() + 1);
}

/// A shorter, lower glyph uses its actual bounds rather than the padded line cell's height.
#[test]
fn badge_tracks_the_fonts_glyph_height_and_vertical_position() {
    let fixture = ui_engine::test_support::fixture_font();
    let mut glyphs = fixture.glyphs().to_vec();
    for glyph in &mut glyphs {
        glyph.uv[3] = glyph.uv[1] + 4;
        glyph.bearing[1] = -6;
    }
    let identity = [9; 32];
    let bytes = assets::encode_font_catalog(identity, &glyphs, fixture.pages()).unwrap();
    let font = RuntimeFontCatalog::decode(&bytes, identity).unwrap();
    let mut tag = anchor("ExamplePlayer");
    tag.set_cinnabar_badge(true);
    let scene = scene_with_font(&[tag], &font);
    let gem = scene.records[2];
    assert_eq!([gem.rect[1], gem.rect[3]], [4.0, 6.0]);
    assert_eq!(gem.rect[2] - gem.rect[0], 2.0);
    assert_eq!(scene.records[1].rect[0] - gem.rect[2], 1.0);
}

/// The gem gets the same through-wall, depth-tested, sneak and cosmetic opacity passes as text.
#[test]
fn badge_shares_text_visibility_and_opacity() {
    for sneaking in [false, true] {
        let mut tag = anchor("ExamplePlayer");
        tag.set_cinnabar_badge(true);
        tag.depth_tested = sneaking;
        tag.sneaking = sneaking;
        tag.text_alpha = if sneaking { DIM_TEXT_ALPHA } else { 1.0 };
        let mut scene = scene(&[tag]);
        let style = serde_json::from_value(serde_json::json!({"opacity": 0.4})).unwrap();
        style_scene(&mut scene, &style, None);
        let glyphs: Vec<_> = scene
            .records
            .iter()
            .filter(|record| record.text == 1)
            .collect();
        for pair in glyphs.as_chunks::<2>().0 {
            assert_eq!(pair[0].color[3], pair[1].color[3]);
            assert_eq!(pair[0].depth_tested, pair[1].depth_tested);
            assert_eq!(pair[0].anchor, pair[1].anchor);
            assert_eq!(pair[0].line_lift, pair[1].line_lift);
            assert_eq!(pair[0].rect[1], pair[1].rect[1]);
        }
        assert_eq!(glyphs.len(), 2);
    }
}

/// Badged tags must still respect the shared GPU draw-record limit for either depth policy.
#[test]
fn badge_records_fit_the_scene_budget() {
    for sneaking in [false, true] {
        let mut tag = anchor("ExamplePlayer");
        tag.set_cinnabar_badge(true);
        tag.depth_tested = sneaking;
        tag.sneaking = sneaking;
        let scene = scene(&vec![tag; MAX_NAMETAG_RECORDS]);
        let per_tag = 3;
        assert_eq!(scene.records.len(), MAX_NAMETAG_RECORDS / per_tag * per_tag);
    }
}

/// Badge display reuses the existing scene buffers' allocation count after atlas warmup.
#[test]
fn badge_adds_no_frame_allocations_or_atlas_uploads() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let mut atlas = NametagAtlas::default();
    let mut tag = anchor("ExamplePlayer");
    tag.set_cinnabar_badge(true);
    let build = |tag: &NametagAnchor, layouts: &mut TextLayoutCache, atlas: &mut NametagAtlas| {
        build_nametag_scene(
            std::slice::from_ref(tag),
            None,
            &font,
            layouts,
            atlas,
            &|page| super::super::super::nametag_atlas::font_page(&font, page),
        )
    };
    let warm = build(&tag, &mut layouts, &mut atlas);
    let (badged, badged_allocs) = test_alloc::count(|| build(&tag, &mut layouts, &mut atlas));
    assert!(Arc::ptr_eq(&warm.atlas, &badged.atlas));
    assert_eq!(warm.atlas_revision, badged.atlas_revision);
    tag.set_cinnabar_badge(false);
    let (_, plain_allocs) = test_alloc::count(|| build(&tag, &mut layouts, &mut atlas));
    assert_eq!(badged_allocs, plain_allocs);
}

/// UUID answers badge both the roster row and the rendered name tag when XUIDs are absent.
#[test]
fn uuid_answer_badges_tab_row_and_name_tag() {
    let uuid = "00112233-4455-6677-8899-aabbccddeeff";
    let mut roster = crate::ui_runtime::player_roster::PlayerRoster::default();
    roster.set_entries(vec![(
        "ExamplePlayer".into(),
        "".into(),
        uuid::Uuid::parse_str(uuid).unwrap().into_bytes(),
    )]);
    roster.set_cinnabar(Arc::new(std::collections::HashSet::from([uuid.into()])));
    assert!(roster.rows()[0].badged);
    let mut tag = anchor("ExamplePlayer");
    tag.set_cinnabar_badge(
        roster.has_badge("", &uuid::Uuid::parse_str(uuid).unwrap().into_bytes()),
    );
    assert!(scene(&[tag]).records.len() > scene(&[anchor("ExamplePlayer")]).records.len());
}
