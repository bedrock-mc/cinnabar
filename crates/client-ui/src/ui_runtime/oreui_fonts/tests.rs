use super::*;
use crate::test_support::fixture_font;
use ui::oreui_theme as theme;

/// Writes a distance-field bundle of `glyphs` bound to the face's shipped manifest.
fn write_bundle(directory: &Path, face: OreUiFont, glyphs: &[assets::GlyphMetrics]) {
    let fixture = fixture_font();
    let profile = face.carrier().font_face.unwrap();
    let manifest = assets::canonical_source_manifest_sha256(profile.manifest);
    let raster = assets::FontFaceRaster {
        em_pixels: NATIVE_EM,
        rendering: assets::FontRendering::NativeSdf,
        carrier: assets::encode_font_catalog(manifest, glyphs, fixture.pages()).unwrap(),
    };
    let bytes = assets::encode_font_face_bundle(manifest, &[raster]).unwrap();
    std::fs::write(directory.join(face.carrier().output), bytes).unwrap();
}

const NATIVE_EM: u32 = 52;

/// Writes the shared Sans carrier the range-limited faces require.
fn write_sans(directory: &Path) {
    let fixture = fixture_font();
    let manifest = assets::canonical_source_manifest_sha256(
        assets::carriers::FONT.font_face.unwrap().manifest,
    );
    let bytes = assets::encode_font_catalog(manifest, fixture.glyphs(), fixture.pages()).unwrap();
    std::fs::write(directory.join(assets::carriers::FONT.output), bytes).unwrap();
}

fn write_face(directory: &Path, face: OreUiFont) {
    write_sans(directory);
    write_bundle(directory, face, fixture_font().glyphs());
}

#[test]
fn faces_stay_uninstalled_without_their_sans_fallback() {
    let directory = tempfile::tempdir().unwrap();
    for face in OreUiFont::ALL {
        write_bundle(directory.path(), face, fixture_font().glyphs());
    }
    let base = fixture_font();
    let fonts = install(Arc::clone(&base), directory.path());
    assert!(Arc::ptr_eq(&base, &fonts));
    write_sans(directory.path());
    let fonts = install(Arc::clone(&base), directory.path());
    assert!(!std::ptr::eq(
        fonts.font_named(OreUiFont::Ten.name()),
        fonts.as_ref()
    ));
}

#[test]
fn theme_roles_resolve_shipped_faces_without_a_game_install() {
    let base = fixture_font();
    let seven = (*base).clone().with_linear_sampling();
    let ten = (*base)
        .clone()
        .with_rendering(assets::FontRendering::NativeCoverage);
    let fonts = base
        .with_named_font("Cinnangles Seven", &seven)
        .unwrap()
        .with_named_font("Cinnangles Ten", &ten)
        .unwrap();
    for role in [theme::BODY, theme::CAPTION, theme::SECONDARY_BUTTON] {
        assert!(fonts.font_named(role.face.name()).linear_sampling());
    }
    for role in [
        theme::HEADER3,
        theme::HEADER5,
        theme::SECTION_HEADER,
        theme::PRIMARY_BUTTON,
    ] {
        assert_eq!(
            fonts.font_named(role.face.name()).rendering(),
            assets::FontRendering::NativeCoverage
        );
    }
}

#[test]
fn compiled_faces_load_without_a_game_bundle_and_preserve_default_glyphs() {
    let directory = tempfile::tempdir().unwrap();
    for face in OreUiFont::ALL {
        write_face(directory.path(), face);
    }
    let base = fixture_font();
    let fonts = install(Arc::clone(&base), directory.path());
    assert_eq!(fonts.glyphs(), base.glyphs());
    // The fixture base is not Sans, so the shared fallback attaches as one more alias.
    assert_eq!(fonts.named_fonts().len(), OreUiFont::ALL.len() + 1);
    for face in OreUiFont::ALL {
        let selected = fonts.font_named(face.name());
        assert_ne!(selected.identity(), base.identity());
        assert_eq!(
            selected.line_metrics(),
            Some(face.carrier().font_face.unwrap().line_metrics_at(NATIVE_EM))
        );
        assert_eq!(selected.rendering(), assets::FontRendering::NativeSdf);
        assert!(usize::from(selected.glyph('/').unwrap().page) >= base.pages().len());
    }
    assert_ne!(
        fonts.font_named(theme::BODY.face.name()).line_metrics(),
        fonts.font_named(theme::HEADER3.face.name()).line_metrics()
    );
}

#[test]
fn missing_optional_carriers_use_the_default_font() {
    let directory = tempfile::tempdir().unwrap();
    let base = fixture_font();
    let fonts = install(Arc::clone(&base), directory.path());
    assert!(Arc::ptr_eq(&base, &fonts));
    for face in OreUiFont::ALL {
        assert!(std::ptr::eq(fonts.font_named(face.name()), fonts.as_ref()));
    }
}

#[test]
fn one_missing_face_does_not_prevent_the_other_from_loading() {
    let directory = tempfile::tempdir().unwrap();
    write_face(directory.path(), OreUiFont::Ten);
    let base = fixture_font();
    let fonts = install(Arc::clone(&base), directory.path());
    assert!(std::ptr::eq(
        fonts.font_named(OreUiFont::Seven.name()),
        fonts.as_ref()
    ));
    assert!(
        fonts
            .font_named(OreUiFont::Ten.name())
            .line_metrics()
            .is_some()
    );
    assert_eq!(fonts.glyphs(), base.glyphs());
}

#[test]
fn corrupt_and_stale_carriers_degrade_without_replacing_valid_faces() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(OreUiFont::Seven.carrier().output);
    write_face(directory.path(), OreUiFont::Ten);
    let base = fixture_font();
    for bytes in [
        b"corrupt".to_vec(),
        assets::encode_font_catalog([7; 32], base.glyphs(), base.pages())
            .unwrap()
            .to_vec(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let fonts = install(Arc::clone(&base), directory.path());
        assert!(std::ptr::eq(
            fonts.font_named(OreUiFont::Seven.name()),
            fonts.as_ref()
        ));
        assert!(
            fonts
                .font_named(OreUiFont::Ten.name())
                .line_metrics()
                .is_some()
        );
    }
}

#[test]
fn installed_shipped_carriers_fit_the_real_ui_and_keep_unicode_glyphs() {
    let directory = std::env::var_os("CINNABAR_FONT_CARRIER_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled")
        });
    if OreUiFont::ALL
        .iter()
        .any(|face| !directory.join(face.carrier().output).is_file())
    {
        eprintln!(
            "skipping installed_shipped_carriers_fit_the_real_ui_and_keep_unicode_glyphs: missing shipped semantic font carrier fixtures; make assets"
        );
        return;
    }
    let base = super::super::presentation::forms::pack_harness::font();
    for face in OreUiFont::ALL {
        assert!(base.named_fonts().contains_key(face.name()));
        let selected = base.font_named(face.name());
        for character in [
            'A', 'a', 'é', 'Ω', 'Я', '中', '日', '→', '⭐', '𝑩', '𝗦', '𝟭', '\u{fffd}',
        ] {
            assert!(
                selected.glyph_source(character).is_some(),
                "{} lacks {character}",
                face.name()
            );
        }
    }
    super::super::presentation::UiPresentationRuntime::new(base).unwrap();
}

#[test]
fn shipped_font_spacing_survives_loading_and_aliasing() {
    let directory = tempfile::tempdir().unwrap();
    let face = OreUiFont::Ten;
    let fixture = fixture_font();
    let glyph = fixture.glyph('0').unwrap();
    let glyphs: Vec<_> = ['.', 'T']
        .into_iter()
        .map(|codepoint| assets::GlyphMetrics {
            codepoint,
            ..*glyph
        })
        .collect();
    write_sans(directory.path());
    write_bundle(directory.path(), face, &glyphs);
    let fonts = install(fixture, directory.path());
    assert_eq!(fonts.font_named(face.name()).kerning_64('T', '.'), -233);
}
