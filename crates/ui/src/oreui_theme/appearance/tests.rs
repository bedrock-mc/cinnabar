use super::*;

#[test]
fn dark_backdrops_reduce_visible_scenery_without_changing_tint_or_opaque_surfaces() {
    for color in [
        [0, 0, 0, 96],
        OVERLAY_SCREEN,
        OVERLAY_MODAL,
        [12, 14, 18, 160],
    ] {
        let dark = Appearance::Dark.backdrop(color);
        assert_eq!(&dark[..3], &color[..3]);
        assert!(dark[3] > color[3]);
        assert_eq!(Appearance::Default.backdrop(color), color);
    }
    for color in [[0, 0, 0, 0], [12, 14, 18, 255]] {
        assert_eq!(Appearance::Dark.backdrop(color), color);
    }
}

fn luminance(color: Rgba) -> f64 {
    let channels = color[..3]
        .iter()
        .map(|channel| {
            let value = f64::from(*channel) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        })
        .collect::<Vec<_>>();
    channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722
}

#[test]
fn dark_mode_keeps_text_readable_in_every_neutral_state() {
    for native in [
        NEUTRAL20, SECONDARY, DISABLED, NEUTRAL80, NEUTRAL, MENU_ITEM,
    ] {
        let role = Appearance::Dark.role(native);
        for background in [role.fill, role.hovered, role.pressed] {
            let contrast = (luminance(role.text) + 0.05) / (luminance(background) + 0.05);
            assert!(
                contrast >= 4.5,
                "{background:?}, {:?}: {contrast}",
                role.text
            );
        }
    }
}

#[test]
fn appearance_preserves_accent_colors_and_opacity() {
    for role in [PRIMARY_ROLE, DESTRUCTIVE] {
        assert_eq!(Appearance::Dark.role(role).fill, role.fill);
        for color in [role.fill, role.hovered, role.pressed] {
            assert_eq!(Appearance::Dark.surface(color), color);
        }
    }
    let mut surface = NEUTRAL20.fill;
    surface[3] = 72;
    assert_eq!(Appearance::Dark.surface(surface)[3], 72);
    let mut text = TEXT_DARK;
    text[3] = 127;
    assert_eq!(Appearance::Dark.ink(text), [255, 255, 255, 127]);
    for color in [TEXT, FIELD_CARET, OVERLAY_SCREEN, OVERLAY_MODAL] {
        assert_eq!(Appearance::Dark.surface(color), color);
    }
}

#[test]
fn default_appearance_is_identity_and_dark_surfaces_are_idempotent() {
    for native in [
        NEUTRAL20, SECONDARY, DISABLED, NEUTRAL80, NEUTRAL, MENU_ITEM,
    ] {
        assert_eq!(Appearance::Default.role(native).fill, native.fill);
        for color in [native.fill, native.hovered, native.pressed, native.text] {
            assert_eq!(Appearance::Default.surface(color), color);
            assert_eq!(Appearance::Default.ink(color), color);
            let dark = Appearance::Dark.surface(color);
            assert_eq!(Appearance::Dark.surface(dark), dark);
        }
    }
}
