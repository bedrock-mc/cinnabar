use assets::{FontPixels, FontTexturePage, GlyphMetrics, RuntimeFontCatalog, encode_font_catalog};
use sha2::{Digest, Sha256};

use super::*;
use crate::ui_runtime::presentation::{SessionGlyphSheets, tests::fixture_font};

fn private_font(marker: u8) -> Arc<RuntimeFontCatalog> {
    let side = UI_LOCAL_FONT_PAGE_SIDE;
    let rgba8 = vec![marker; side as usize * side as usize * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/private-font-fixture.png".into(),
        source_bytes: 1,
        source_sha256: [marker; 32],
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: side,
        height: side,
        pixels: FontPixels::Rgba8(rgba8),
    };
    let metrics = GlyphMetrics {
        codepoint: 'A',
        page: 0,
        uv: [0, 0, 4, 8],
        bearing: [0, 0],
        advance_64: 6 * 64,
    };
    let bytes = encode_font_catalog([marker; 32], &[metrics], &[page]).unwrap();
    let font = RuntimeFontCatalog::decode(&bytes, [marker; 32])
        .unwrap()
        .with_linear_sampling();
    Arc::new(font.with_glyphs(
        &[SheetGlyph {
            metrics,
            draw_size_64: [2 * 64, 4 * 64],
        }],
        |_| true,
    ))
}

#[test]
fn late_font_replaces_one_page_without_reallocating_static_or_server_slots() {
    let base = fixture_font();
    let mut runtime = UiPresentationRuntime::new(Arc::clone(&base)).unwrap();
    let textures = runtime.textures.clone();
    let font = private_font(19);
    runtime.set_mod_panel_font(Some(font.clone())).unwrap();
    let target = textures.dynamic_start() + UI_LOCAL_FONT_PAGE_OFFSET;
    assert_eq!(
        runtime.textures.static_identity(),
        textures.static_identity()
    );
    assert_eq!(runtime.textures.plan(), textures.plan());
    assert_eq!(runtime.textures.pages().len(), textures.pages().len());
    let changed = runtime
        .textures
        .pages()
        .iter()
        .zip(textures.pages())
        .enumerate()
        .filter_map(|(index, (current, old))| {
            (current.identity() != old.identity()).then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(changed, [target]);
    assert_eq!(runtime.font.glyphs(), base.glyphs());
    let alias = runtime.font.font_named(ui::mod_panel::FONT_NAME);
    assert_eq!(usize::from(alias.glyph('A').unwrap().page), target);
    assert_eq!(alias.draw_size_64('A'), font.draw_size_64('A'));
    assert!(alias.linear_sampling());
    assert!(!runtime.font.linear_sampling());
    let installed = runtime.textures.clone();
    runtime.set_mod_panel_font(Some(font)).unwrap();
    assert!(Arc::ptr_eq(&runtime.textures, &installed));
    runtime.set_mod_panel_font(None).unwrap();
    assert_eq!(runtime.textures.identity(), textures.identity());
    assert_eq!(runtime.font.identity(), base.identity());
    assert!(Arc::ptr_eq(&runtime.base_font, &base));
}

#[test]
fn late_font_and_unload_keep_live_server_glyphs_and_other_named_fonts() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    let cell = assets::CellGlyph {
        codepoint: 'A',
        size: [3, 4],
        bearing: [0, 0],
        advance_64: 4 * 64,
        draw_size_64: [3 * 64, 4 * 64],
        rgba8: vec![255; 3 * 4 * 4].into(),
    };
    let sheets = Arc::new(SessionGlyphSheets::with_named(
        vec![cell.clone()],
        std::collections::BTreeMap::from([
            ("server-font".into(), vec![cell.clone()]),
            (ui::mod_panel::FONT_NAME.into(), vec![cell]),
        ]),
    ));
    session_glyphs::observe(&mut runtime, Some(&sheets));
    let default = *runtime.font.glyph('A').unwrap();
    let server = runtime.font.font_named("server-font").identity();
    let overridden = runtime.font.font_named(ui::mod_panel::FONT_NAME).identity();
    let page = runtime.session_glyphs.pages[0].clone();
    runtime.set_mod_panel_font(Some(private_font(23))).unwrap();
    assert_eq!(*runtime.font.glyph('A').unwrap(), default);
    assert_eq!(runtime.font.font_named("server-font").identity(), server);
    assert_ne!(
        runtime.font.font_named(ui::mod_panel::FONT_NAME).identity(),
        overridden
    );
    assert_eq!(runtime.session_glyphs.pages[0].identity(), page.identity());
    session_glyphs::observe(&mut runtime, None);
    assert!(
        runtime
            .font
            .named_fonts()
            .contains_key(ui::mod_panel::FONT_NAME)
    );
    session_glyphs::observe(&mut runtime, Some(&sheets));
    runtime.set_mod_panel_font(None).unwrap();
    assert_eq!(*runtime.font.glyph('A').unwrap(), default);
    assert_eq!(runtime.font.font_named("server-font").identity(), server);
    assert_eq!(
        runtime.font.font_named(ui::mod_panel::FONT_NAME).identity(),
        overridden
    );
}

#[test]
fn invalid_late_font_retains_the_installed_catalog_and_pixels() {
    let mut runtime = UiPresentationRuntime::new(fixture_font()).unwrap();
    runtime.set_mod_panel_font(Some(private_font(37))).unwrap();
    let font = runtime.font.clone();
    let textures = runtime.textures.clone();
    assert!(runtime.set_mod_panel_font(Some(fixture_font())).is_err());
    let source = private_font(41);
    let mut metrics = *source.glyph('A').unwrap();
    metrics.uv = [2, 2, 1, 1];
    let invalid = Arc::new(source.with_glyphs(
        &[SheetGlyph {
            metrics,
            draw_size_64: [1; 2],
        }],
        |_| true,
    ));
    assert!(runtime.set_mod_panel_font(Some(invalid)).is_err());
    assert!(Arc::ptr_eq(&runtime.font, &font));
    assert!(Arc::ptr_eq(&runtime.textures, &textures));
}
