//! Player-card geometry from OreUI `bZ` and `hZ` (1.26.50.04).
use super::super::icons::{self, Icon};
use super::super::theme::{BORDER, CAPTION, HEADER5, NEUTRAL100, TEXT_DIMMER, TEXT_DIMMEST};
use super::*;

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
    let text_height = canvas.measure_height(name, name_width, HEADER5)?
        + canvas.measure_height(status, name_width, CAPTION)?
        + space(canvas, 2);
    let name_height = text_height.max(pic);
    let name_top = if narrow {
        b[1] + canvas.r(0.8)
    } else {
        b[1] + banner_height + space(canvas, 2)
    };
    let button_y = name_top + name_height + space(canvas, 2);
    let end = if narrow {
        b[1] + canvas.r(12.8)
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
            ui::profile_banner_index(&profile.xuid, banners.len()).unwrap_or_default();
        if let Some(originals) = canvas.originals
            && let Some(&uv) = originals.sprites.get(banners[banner_index])
        {
            let icon = IconRef {
                page: originals.page,
                uv,
                glint: false,
            };
            canvas.icon_ref(cover(icon, banner_bounds), banner_bounds)?;
        }
    }
    if let Some(icon) = character {
        let model_width = canvas.r(14.8);
        let model_height = canvas.r(19.6);
        let left = b[0] + (banner_width - model_width) * 0.5 - canvas.r(4.0);
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
    let h = canvas.text(name, [name_left, text_y], name_width, HEADER5, TEXT, false)?;
    canvas.text(
        status,
        [name_left, text_y + h],
        name_width,
        CAPTION,
        TEXT_DIMMER,
        false,
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
        None,
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
