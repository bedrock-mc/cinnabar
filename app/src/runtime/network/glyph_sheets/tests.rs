use std::io::Write;

use resource_pack::LayeredPackView;

use super::compile_session_glyphs;

fn sheet_png(cell: u32, opaque_cell: u32) -> Vec<u8> {
    let side = cell * 16;
    let image = image::RgbaImage::from_fn(side, side, |x, y| {
        let index = (y / cell) * 16 + x / cell;
        image::Rgba([255, 255, 255, if index == opaque_cell { 255 } else { 0 }])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn archive(id: u8, files: &[(&str, Vec<u8>)]) -> protocol::ResourcePackArchive {
    let id = format!("00000000-0000-0000-0000-{id:012}");
    let manifest = format!(
        r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
    );
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (path, bytes) in std::iter::once(("manifest.json", manifest.into_bytes()))
        .chain(files.iter().map(|(path, bytes)| (*path, bytes.clone())))
    {
        writer
            .start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    protocol::ResourcePackArchive::unencrypted(
        id.parse().unwrap(),
        "1.0.0".into(),
        String::new(),
        writer.finish().unwrap().into_inner(),
    )
}

fn view(archives: Vec<protocol::ResourcePackArchive>) -> LayeredPackView {
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(archives),
    ))
}

#[test]
fn a_stack_without_glyph_sheets_yields_none() {
    assert!(compile_session_glyphs(&view(vec![archive(1, &[])])).is_none());
}

#[test]
fn empty_stack_records_missing_sheets_for_later_activation() {
    let view = LayeredPackView::tracked(view(vec![]).shared_stack());
    assert!(compile_session_glyphs(&view).is_none());
    let inputs = view.dependencies().unwrap().snapshot();
    for path in [
        "font/default8.png",
        "font/ascii_sga.png",
        "font/glyph_E0.png",
    ] {
        assert!(inputs.contains(&resource_pack::PackDependency::File {
            path: path.into(),
            limit: super::MAX_SHEET_SOURCE_BYTES,
        }));
    }
}

// The last-applied pack replaces a sheet wholesale; sheets it lacks still come from lower packs.
#[test]
fn the_last_applied_pack_wins_per_sheet() {
    let sheets = compile_session_glyphs(&view(vec![
        archive(
            1,
            &[
                ("font/glyph_E0.png", sheet_png(8, 1)),
                ("font/glyph_E1.png", sheet_png(8, 2)),
            ],
        ),
        archive(2, &[("font/glyph_E0.png", sheet_png(16, 3))]),
    ]))
    .expect("sheets");
    let cell = |c: char| {
        sheets
            .cells
            .iter()
            .find(|cell| cell.codepoint == c)
            .unwrap()
    };
    assert_eq!(sheets.cells.len(), 512);
    // E0 comes from the top pack (16 px cells, cell 3 opaque); E1 falls through to the lower one.
    assert_eq!(cell('\u{e003}').size, [16, 16]);
    assert_eq!(cell('\u{e002}').size, [0, 0]);
    assert_eq!(cell('\u{e102}').size, [8, 8]);
}

// Local-only: set CINNABAR_SERVER_PACK to a cached server `.mcpack` to check its sheets decode and pack.
#[test]
fn a_real_server_pack_decodes_and_packs() {
    let Some(view) = crate::runtime::network::local_pack::local_pack_view("CINNABAR_SERVER_PACK")
    else {
        eprintln!(
            "skipping a_real_server_pack_decodes_and_packs: fixture unavailable; requires CINNABAR_SERVER_PACK naming an offline pack with glyph sheets"
        );
        return;
    };
    let Some(sheets) = compile_session_glyphs(&view) else {
        eprintln!("no glyph sheets");
        eprintln!(
            "skipping a_real_server_pack_decodes_and_packs: fixture unavailable; requires CINNABAR_SERVER_PACK naming an offline pack with glyph sheets"
        );
        return;
    };
    let atlas = assets::pack_cells(&sheets.cells, 0, 256, 8);
    assert!(!atlas.glyphs.is_empty());
    eprintln!(
        "{} cells, {} glyphs, {} pages",
        sheets.cells.len(),
        atlas.glyphs.len(),
        atlas.pages.len()
    );
}

#[test]
fn default8_maps_ascii_keeps_unicode_and_restores_the_lower_sheet() {
    let lower = || archive(1, &[("font/default8.png", sheet_png(8, u32::from('A')))]);
    let upper = archive(
        2,
        &[
            ("font/default8.png", sheet_png(16, u32::from('B'))),
            ("font/glyph_00.png", sheet_png(8, u32::from('é'))),
        ],
    );
    let applied = compile_session_glyphs(&view(vec![lower(), upper])).unwrap();
    let glyph = |codepoint| {
        applied
            .cells
            .iter()
            .find(|cell| cell.codepoint == codepoint)
            .unwrap()
    };
    assert_eq!(glyph('A').size, [0, 0]);
    assert_eq!(glyph('B').size, [16, 16]);
    assert_eq!(
        glyph('é').size,
        [0, 0],
        "extended characters use default8 when its mapping includes them"
    );
    assert_eq!(
        applied
            .cells
            .iter()
            .filter(|cell| cell.codepoint == 'B')
            .count(),
        1
    );
    assert_eq!(
        glyph(' ').advance_64,
        (assets::texel_size_64(0, 1) / 2) as i16
    );
    let restored = compile_session_glyphs(&view(vec![lower()])).unwrap();
    assert_eq!(restored.cells.len(), 256);
    assert_eq!(
        restored
            .cells
            .iter()
            .find(|cell| cell.codepoint == 'A')
            .unwrap()
            .size,
        [8, 8]
    );
}

#[test]
fn default8_keeps_leading_padding_in_advance_and_falls_through_bad_images() {
    let image = image::RgbaImage::from_fn(128, 128, |x, y| {
        let cell = (y / 8) * 16 + x / 8;
        let opaque =
            cell == u32::from('A') && (2..5).contains(&(x % 8)) && (1..7).contains(&(y % 8));
        image::Rgba([240, 120, 60, if opaque { 255 } else { 0 }])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let applied = compile_session_glyphs(&view(vec![
        archive(1, &[("font/default8.png", bytes.into_inner())]),
        archive(2, &[("font/default8.png", b"malformed png".to_vec())]),
    ]))
    .unwrap();
    let glyph = applied
        .cells
        .iter()
        .find(|cell| cell.codepoint == 'A')
        .unwrap();
    let texel = assets::texel_size_64(0, 8);
    assert_eq!(glyph.size, [3, 6]);
    assert_eq!(glyph.bearing[0], (2 * texel / 64) as i16);
    assert_eq!(glyph.advance_64, (6 * texel) as i16);
    assert!(
        glyph
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [240, 120, 60, 255])
    );
}

#[test]
fn extended_mapping_named_bitmap_and_locale_overrides_are_kept_separate() {
    let metadata = br#"{"version":1,"fonts":[{"font_format":"bitmap","font_name":"custom","ascii_font_file":"font/custom"}],"font_aliases":[{"alias":"rune","fonts":[{"font_reference":"custom","font_ranges":[{"first":65,"last":65}],"font_language_code":"en_US"}]}]}"#;
    let sheets = compile_session_glyphs(&view(vec![archive(
        1,
        &[
            ("font/default8.png", sheet_png(8, 130)),
            ("font/custom.png", sheet_png(8, 65)),
            ("font/font_metadata.json", metadata.to_vec()),
            ("texts/en_US/font/glyph_E0.png", sheet_png(16, 1)),
            ("font/glyph_E0.png", sheet_png(8, 1)),
        ],
    )]))
    .unwrap();
    assert_eq!(
        sheets
            .cells
            .iter()
            .find(|cell| cell.codepoint == 'é')
            .unwrap()
            .size,
        [8, 8]
    );
    assert_eq!(
        sheets
            .cells
            .iter()
            .find(|cell| cell.codepoint == '\u{e001}')
            .unwrap()
            .size,
        [16, 16]
    );
    assert_eq!(sheets.named["rune"].len(), 1);
    assert_eq!(sheets.named["rune"][0].codepoint, 'A');
    assert_eq!(sheets.named["rune"][0].size, [8, 8]);
}
