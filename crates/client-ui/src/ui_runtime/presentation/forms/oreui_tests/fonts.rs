//! The shipped OreUI faces installed into the presentation's font.

use std::path::Path;

use super::super::super::UiPresentationRuntime;
use ui::oreui_theme::OreUiFont;

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
    let base = super::super::pack_harness::font();
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
    UiPresentationRuntime::new(base).unwrap();
}
