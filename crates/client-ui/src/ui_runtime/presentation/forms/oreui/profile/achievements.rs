//! Overview achievement sections from OreUI `b2`, `Ik`, `Rk`, `xk`, and `gK`.
use super::super::theme::{
    BODY, BORDER, CAPTION, INFORMATIVE_TINT, NEUTRAL, SUCCESS_TINT, TEXT_DARK, TEXT_DIMMER,
};
use protocol::launcher_control::ProfileAchievement;
use std::collections::HashMap;
use {super::*, launcher::menu::MenuView, ui::IconRef};

/// Draws known suggestions and the three most recently completed achievements.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    artwork: &HashMap<String, IconRef>,
) -> Result<f32, UiPresentationError> {
    let profile = &view.feeds.profile;
    if !profile.achievements_loaded {
        super::loading(canvas, b)?;
        return Ok(b[1] + canvas.r(9.2));
    }
    if profile.achievements_error {
        return Ok(b[1]);
    }
    let Some(data) = &profile.achievements else {
        return Ok(b[1]);
    };
    let [suggested, completed] =
        launcher::menu::profile_achievements::visible_achievements(&data.entries);
    let mut y = b[1];
    for (title, entries, tint) in [
        ("Suggested next achievements", suggested, INFORMATIVE_TINT),
        ("Recently completed achievements", completed, SUCCESS_TINT),
    ] {
        let label_width = (canvas.measure(title, CAPTION)? + canvas.r(1.6)).min(b[2] - b[0]);
        canvas.fill([b[0], y, b[0] + label_width, y + canvas.r(3.0)], tint)?;
        canvas.frame(
            [b[0], y, b[0] + label_width, y + canvas.r(3.0)],
            0.2,
            BORDER,
        )?;
        canvas.fill(
            [
                b[0] + label_width - canvas.r(0.2),
                y + canvas.r(2.2),
                b[2],
                y + canvas.r(3.0),
            ],
            tint,
        )?;
        canvas.frame(
            [
                b[0] + label_width - canvas.r(0.2),
                y + canvas.r(2.2),
                b[2],
                y + canvas.r(3.0),
            ],
            0.2,
            BORDER,
        )?;
        canvas.text(
            title,
            [b[0] + canvas.r(0.8), y + canvas.r(0.2)],
            label_width - canvas.r(1.6),
            CAPTION,
            TEXT_DARK,
            false,
        )?;
        y += canvas.r(2.8);
        for entry in entries {
            y = achievement(canvas, entry, [b[0], y, b[2], b[3]], artwork)?;
        }
        y += space(canvas, 2);
    }
    Ok(y)
}

/// Draws the service title, description, art and Minecraft gamerscore for one achievement.
fn achievement(
    canvas: &mut Canvas<'_>,
    entry: &ProfileAchievement,
    b: Bounds,
    artwork: &HashMap<String, IconRef>,
) -> Result<f32, UiPresentationError> {
    let left = b[0] + canvas.r(10.4);
    let score = entry
        .gamerscore
        .filter(|score| *score > 0)
        .map(|n| n.to_string());
    let score_width = score
        .as_ref()
        .map(|s| canvas.measure(s, CAPTION))
        .transpose()?
        .unwrap_or_default();
    let width = (b[2] - left - canvas.r(2.4) - score_width).max(canvas.r(1.0));
    let name_height = canvas.measure_height(&entry.name, width, BODY)?;
    let description_height = canvas.measure_height(&entry.description, width, CAPTION)?;
    let height = (name_height + description_height + canvas.r(1.6)).max(canvas.r(7.2));
    canvas.fill([b[0], b[1], b[2], b[1] + height], NEUTRAL.fill)?;
    canvas.frame([b[0], b[1], b[2], b[1] + height], 0.2, BORDER)?;
    if let Some(icon) = artwork.get(&entry.image.path) {
        let y = b[1] + (height - canvas.r(5.2)) * 0.5;
        canvas.icon_ref(
            *icon,
            [
                b[0] + canvas.r(0.8),
                y,
                b[0] + canvas.r(9.6),
                y + canvas.r(5.2),
            ],
        )?;
    }
    let y = b[1] + (height - name_height - description_height) * 0.5;
    canvas.text(&entry.name, [left, y], width, BODY, TEXT, false)?;
    canvas.text(
        &entry.description,
        [left, y + name_height],
        width,
        CAPTION,
        TEXT_DIMMER,
        false,
    )?;
    if let Some(score) = score {
        canvas.text(
            &score,
            [
                b[2] - canvas.r(0.8) - score_width,
                b[1] + (height - canvas.r(CAPTION.line)) * 0.5,
            ],
            score_width + 1.0,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
    }
    Ok(b[1] + height - canvas.r(0.2))
}
