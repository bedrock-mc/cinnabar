use super::*;
use crate::{oreui_theme as theme, test_support::fixture_font};

/// Writes a small valid carrier bound to the selected shipped manifest.
fn write_face(directory: &Path, face: OreUiFont) {
    let fixture = fixture_font();
    let profile = face.carrier().font_face.unwrap();
    let manifest = assets::canonical_source_manifest_sha256(profile.manifest);
    let bytes = assets::encode_font_catalog(manifest, fixture.glyphs(), fixture.pages()).unwrap();
    std::fs::write(directory.join(face.carrier().output), bytes).unwrap();
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
    assert_eq!(fonts.named_fonts().len(), OreUiFont::ALL.len());
    for face in OreUiFont::ALL {
        let selected = fonts.font_named(face.name());
        assert_ne!(selected.identity(), base.identity());
        assert_eq!(
            selected.line_metrics(),
            Some(face.carrier().font_face.unwrap().line_metrics())
        );
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
            "skipping installed_shipped_carriers_fit_the_real_ui_and_keep_unicode_glyphs: missing Cinnangles Seven/Ten carrier fixtures; make font-seven-assets font-ten-assets"
        );
        return;
    }
    let base = super::super::presentation::forms::pack_harness::font();
    for face in OreUiFont::ALL {
        assert!(base.named_fonts().contains_key(face.name()));
        let selected = base.font_named(face.name());
        for character in ['A', 'a', 'é', 'Ω', 'Я', '中', '日', '→', '\u{fffd}'] {
            assert!(
                selected.glyph(character).is_some(),
                "{} lacks {character}",
                face.name()
            );
        }
    }
    super::super::presentation::UiPresentationRuntime::new(base).unwrap();
}
