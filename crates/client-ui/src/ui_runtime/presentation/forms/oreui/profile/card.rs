//! Player-card geometry from OreUI `bZ` and `hZ` (1.26.50.04).
use super::super::icons::{self, Icon};
use super::super::theme::{BORDER, CAPTION, HEADER5, NEUTRAL100, TEXT_DIMMER, TEXT_DIMMEST};
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
    ui::IconRef,
};

/// Fixed height of OreUI's narrow player card (CSS `.cfdfb462a95f1a51c994`).
const NARROW_CARD_HEIGHT_REM: f32 = 12.8;

/// Draws a natural-height player card; the narrow form places its banner on the right.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    portrait: Option<IconRef>,
    avatar: Option<IconRef>,
    featured: Option<IconRef>,
    narrow: bool,
) -> Result<f32, UiPresentationError> {
    let profile = &view.feeds.profile;
    let name = if profile.gamertag.is_empty() {
        &view.display_name
    } else {
        &profile.gamertag
    };
    let status = if profile.real_name.is_empty() {
        &profile.presence
    } else {
        &profile.real_name
    };
    let pad = canvas.r(if narrow { 0.8 } else { 1.2 });
    let pic = canvas.r(5.2);
    let character = avatar.filter(|_| !narrow);
    let banner_width = (b[2] - b[0]) * if narrow { 0.4 } else { 1.0 };
    let banner_height = banner_width * 9.0 / 16.0;
    let content_right = if narrow {
        b[2] - banner_width - canvas.r(0.8)
    } else {
        b[2] - pad
    };
    let name_left = b[0] + pad + pic + space(canvas, 2);
    let name_width = (content_right - name_left).max(canvas.r(1.0));
    // OreUI vZ wraps each label in yu: nowrap with an overflow ellipsis.
    let text_height = canvas.r(HEADER5.line + CAPTION.line) + space(canvas, 2);
    let name_height = text_height.max(pic);
    let name_top = if narrow {
        b[1] + canvas.r(0.8)
    } else {
        b[1] + banner_height + space(canvas, 2)
    };
    let button_y = name_top + name_height + space(canvas, 2);
    let end = if narrow {
        b[1] + canvas.r(NARROW_CARD_HEIGHT_REM)
    } else {
        button_y + canvas.r(4.4) + pad
    };
    canvas.fill([b[0], b[1], b[2], end], NEUTRAL80.fill)?;
    canvas.frame([b[0], b[1], b[2], end], 0.2, BORDER)?;
    let banner_left = if narrow { b[2] - banner_width } else { b[0] };
    let banner_bottom = if narrow {
        end - canvas.r(0.2)
    } else {
        b[1] + banner_height
    };
    canvas.fill(
        [
            banner_left + canvas.r(0.2),
            b[1] + canvas.r(0.2),
            b[2] - canvas.r(0.2),
            banner_bottom,
        ],
        NEUTRAL100,
    )?;
    let banner_bounds = [
        banner_left + canvas.r(0.2),
        b[1] + canvas.r(0.2),
        b[2] - canvas.r(0.2),
        banner_bottom,
    ];
    if let Some(icon) = featured {
        canvas.icon_ref(cover(icon, banner_bounds), banner_bounds)?;
    } else {
        let banners = &crate::ui_runtime::oreui_assets::PROFILE_BANNERS;
        let banner_index =
            launcher::menu::profile_banner_index(&profile.xuid, banners.len()).unwrap_or_default();
        if let Some(originals) = canvas.originals
            && let Some(sprite) = originals.sprites.get(banners[banner_index])
        {
            let icon = IconRef {
                page: originals.page + sprite.page,
                uv: sprite.bounds,
                glint: false,
            };
            canvas.icon_ref(cover(icon, banner_bounds), banner_bounds)?;
        }
    }
    if let Some(icon) = character {
        let model_width = canvas.r(14.8);
        let model_height = canvas.r(19.6);
        let left = b[0] - canvas.r(4.0);
        let bottom = b[1] + banner_height + canvas.r(7.6);
        canvas.icon_ref(
            icon,
            [left, bottom - model_height, left + model_width, bottom],
        )?;
    }
    let pic_bounds = if character.is_some() {
        [
            b[2] - pad - pic,
            b[1] + banner_height - pic + canvas.r(0.8),
            b[2] - pad,
            b[1] + banner_height + canvas.r(0.8),
        ]
    } else {
        [b[0] + pad, name_top, b[0] + pad + pic, name_top + pic]
    };
    if let Some(icon) = portrait {
        canvas.icon_ref(icon, pic_bounds)?;
    } else if character.is_none() {
        canvas.fill(pic_bounds, NEUTRAL100)?;
        icons::draw(
            canvas,
            Icon::Player,
            [pic_bounds[0] + canvas.r(1.8), pic_bounds[1] + canvas.r(1.6)],
            TEXT_DIMMEST,
        )?;
    }
    let text_y = name_top + space(canvas, 1);
    canvas.text_line(name, [name_left, text_y], name_width, HEADER5, TEXT)?;
    canvas.text_line(
        status,
        [name_left, text_y + canvas.r(HEADER5.line)],
        name_width,
        CAPTION,
        TEXT_DIMMER,
    )?;
    button(
        canvas,
        view,
        [
            b[0] + pad,
            button_y,
            content_right,
            button_y + canvas.r(4.4),
        ],
        Variant::Secondary,
        "Dressing room",
        Some(MenuAction::Navigate(
            launcher::menu::MenuScreen::DressingRoom,
        )),
    )?;
    Ok(end)
}

/// Crops image UVs around their center, matching OreUI `lm` background-size cover.
fn cover(mut icon: IconRef, bounds: Bounds) -> IconRef {
    let [left, top, right, bottom] = icon.uv;
    let width = f32::from(right - left);
    let height = f32::from(bottom - top);
    let aspect = (bounds[2] - bounds[0]) / (bounds[3] - bounds[1]);
    if width <= 0.0 || height <= 0.0 || !aspect.is_finite() || aspect <= 0.0 {
        return icon;
    }
    if width / height > aspect {
        let crop = ((width - height * aspect) * 0.5).round() as u16;
        icon.uv[0] += crop;
        icon.uv[2] -= crop;
    } else {
        let crop = ((height - width / aspect) * 0.5).round() as u16;
        icon.uv[1] += crop;
        icon.uv[3] -= crop;
    }
    icon
}

#[cfg(test)]
mod tests {
    use crate::ui_runtime::presentation::TextMetrics;
    use crate::ui_runtime::presentation::tests::fixture_font;
    use {
        super::*,
        launcher::menu::{MenuAction, MenuView},
        ui::IconRef,
    };

    #[test]
    fn wide_card_character_stays_beside_the_name_when_the_banner_widens() {
        let view = MenuView::new(true, "BugTest".into());
        let character = IconRef {
            page: 7,
            uv: [0, 0, 148, 196],
            glint: false,
        };
        let mut positions = Vec::new();
        for width in [300.0, 500.0] {
            let (_, _, nodes) = super::super::super::review_tests::paint(
                std::collections::HashMap::new(),
                |canvas| {
                    draw(
                        canvas,
                        &view,
                        [100.0, 100.0, 100.0 + width, 700.0],
                        None,
                        Some(character),
                        None,
                        false,
                    )
                    .unwrap();
                },
            );
            let image = nodes
                .iter()
                .find(|node| {
                    matches!(
                        node.visual(),
                        ui::UiVisual::Sprite {
                            texture_page: 7,
                            ..
                        }
                    )
                })
                .unwrap();
            positions.push(image.bounds().min().x());
        }
        assert_eq!(
            positions[0], positions[1],
            "wider banners must not move the character into the name column"
        );
    }

    #[test]
    fn self_card_offers_a_dressing_room_action_in_both_layouts() {
        let view = MenuView::new(true, "BugTest".into());
        for narrow in [false, true] {
            let (_, hits, _) = super::super::super::review_tests::paint(
                std::collections::HashMap::new(),
                |canvas| {
                    draw(
                        canvas,
                        &view,
                        [0.0, 0.0, 600.0, 700.0],
                        None,
                        None,
                        None,
                        narrow,
                    )
                    .unwrap();
                },
            );
            assert!(hits.iter().any(|(action, _)| *action
                == MenuAction::Navigate(launcher::menu::MenuScreen::DressingRoom)));
        }
    }

    #[test]
    fn narrow_card_ellipsizes_long_names_without_moving_its_button() {
        let mut view = MenuView::new(true, "Fixture".into());
        view.feeds.profile.gamertag = "Long player name ".repeat(20);
        view.feeds.profile.real_name = "Long real name ".repeat(20);
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
        let font = fixture_font();
        let metrics =
            TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        let bounds = [0.0, 0.0, canvas.r(50.0), canvas.r(40.0)];
        let end = draw(&mut canvas, &view, bounds, None, None, None, true).unwrap();
        let card_bottom = canvas.r(NARROW_CARD_HEIGHT_REM);
        let line_limit = canvas.r(HEADER5.line);
        drop(canvas);
        assert_eq!(end, card_bottom);
        let mut ellipsis_labels = 0;
        for node in &nodes {
            assert!(
                node.bounds().max().y() <= end,
                "card content escaped its fixed height"
            );
            if let ui::UiVisual::Text { layout, .. } = node.visual() {
                assert!(
                    node.bounds().height() <= line_limit,
                    "Profile text wrapped instead of ellipsizing"
                );
                assert_eq!(layout.line_count(), 1);
                if layout
                    .glyphs()
                    .last()
                    .is_some_and(|glyph| glyph.codepoint == '…')
                {
                    ellipsis_labels += 1;
                }
            }
        }
        assert_eq!(
            ellipsis_labels, 2,
            "both player name and real name need ellipses"
        );
    }
}
