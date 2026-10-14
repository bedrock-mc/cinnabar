use super::*;

/// Original tiny RGBA artwork exercises the actual PNG decoder without game or product assets.
fn png(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba(color))
        .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

#[test]
fn checked_logo_and_heading_override_main_menu_and_reset_to_original_art() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation.finish_menu_artwork();
    let original = presentation.home_title_icon().unwrap();
    let bytes = png(12, 4, [19, 53, 79, 255]);
    let branding = Arc::new(
        PreparedMenuBranding::prepare(Some(&bytes), Some("LOCAL"), [4096, 2048], 4 * 1024 * 1024)
            .unwrap(),
    );
    presentation.set_mod_menu_branding(Some(branding));
    assert_eq!(
        presentation.home_title_icon(),
        Some(original),
        "pending artwork retains the shipped logo"
    );
    presentation.finish_menu_artwork();
    let overridden = presentation.home_title_icon().unwrap();
    assert_eq!(
        [
            overridden.uv[2] - overridden.uv[0],
            overridden.uv[3] - overridden.uv[1]
        ],
        [12, 4]
    );
    assert_eq!(presentation.mod_menu_title(), Some("LOCAL"));
    let page = &presentation.textures.pages()[usize::from(overridden.page)];
    let first = ((usize::from(overridden.uv[1]) * page.dimensions()[0] as usize)
        + usize::from(overridden.uv[0]))
        * 4;
    assert_eq!(&page.pixels()[first..first + 4], &[19, 53, 79, 255]);
    presentation.set_mod_menu_branding(None);
    assert!(presentation.mod_menu_logo().is_none());
    assert!(presentation.mod_menu_title().is_none());
    assert_eq!(presentation.home_title_icon(), Some(original));
}

#[test]
fn invalid_large_or_non_png_logos_fail_before_presentation_publication() {
    assert!(
        PreparedMenuBranding::prepare(Some(b"not PNG"), None, [4096, 2048], 4 * 1024 * 1024)
            .is_err()
    );
    assert!(PreparedMenuBranding::prepare(Some(&png(8, 4, [0; 4])), None, [7, 4], 32).is_err());
    assert!(PreparedMenuBranding::prepare(Some(&png(8, 4, [0; 4])), None, [8, 4], 31).is_err());
    assert!(
        PreparedMenuBranding::prepare(None, Some("two\nlines"), [4096, 2048], 4 * 1024 * 1024)
            .is_err()
    );
}

#[test]
fn replacement_sources_supersede_pending_logo_and_preserve_native_aspect() {
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let first = png(12, 4, [91, 31, 11, 255]);
    let second = png(2043, 770, [19, 53, 79, 255]);
    let prepare = |bytes: &[u8], title| {
        Arc::new(
            PreparedMenuBranding::prepare(Some(bytes), Some(title), [4096, 2048], 4 * 1024 * 1024)
                .unwrap(),
        )
    };
    presentation.set_mod_menu_branding(Some(prepare(&first, "FIRST")));
    presentation.set_mod_menu_branding(Some(prepare(&second, "SECOND")));
    presentation.finish_menu_artwork();
    assert_eq!(presentation.mod_menu_title(), Some("SECOND"));
    let icon = presentation.home_title_icon().unwrap();
    let size = [icon.uv[2] - icon.uv[0], icon.uv[3] - icon.uv[1]];
    assert!(size[0] <= ui_engine::pages::WHOLE_ART_PAGE as u16);
    let ratio = f32::from(size[0]) / f32::from(size[1]);
    assert!((ratio - 2043.0 / 770.0).abs() < 0.005);
    let old = presentation.textures.clone();
    presentation.set_mod_menu_branding(Some(prepare(&second, "SECOND")));
    assert!(
        Arc::ptr_eq(&old, &presentation.textures),
        "same source preserves resident pages"
    );
}
