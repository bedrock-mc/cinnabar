//! Profile summary and statistic rows from OreUI `g2` / `h2` / `EJ`.
use super::super::theme::{BODY, BORDER, CAPTION, NEUTRAL, TEXT_DIMMER, TEXT_DIMMEST};
use super::super::widgets::Interaction;
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
};

/// Draws the combined social summary and the available Overview service summaries.
pub(super) fn overview(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
) -> Result<f32, UiPresentationError> {
    let p = &view.feeds.profile;
    let middle = (b[0] + b[2]) * 0.5;
    summary(
        canvas,
        view,
        [b[0], b[1], middle, b[1] + canvas.r(7.2)],
        "Friends",
        p.friends.map(|n| n.to_string()).as_deref(),
        p.friends
            .filter(|n| *n > 0)
            .map(|_| MenuAction::Navigate(launcher::menu::MenuScreen::Friends)),
    )?;
    summary(
        canvas,
        view,
        [middle - canvas.r(0.2), b[1], b[2], b[1] + canvas.r(7.2)],
        "Followers",
        p.followers.map(|n| n.to_string()).as_deref(),
        None,
    )?;
    let mut y = b[1] + canvas.r(7.0);
    // A gallery count is unknown until its local/cloud gallery carrier exists.
    summary(
        canvas,
        view,
        [b[0], y, b[2], y + canvas.r(7.2)],
        "Screenshot gallery",
        None,
        None,
    )?;
    y += canvas.r(7.0);
    let achievements = p.achievements.as_ref();
    let unlocked = achievements.map(|a| format!("{} / {}", a.unlocked, a.total));
    summary(
        canvas,
        view,
        [b[0], y, b[2], y + canvas.r(7.2)],
        "Achievements",
        unlocked.as_deref(),
        None,
    )?;
    if let Some(score) = achievements
        .and_then(|a| Some(format!("{} / {}", a.current_gamerscore?, a.max_gamerscore?)))
    {
        let width = canvas.measure(&score, CAPTION)?;
        let x = b[2] - canvas.r(0.8) - width;
        canvas.text(
            &score,
            [x, y + canvas.r(2.6)],
            width + 1.0,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
        let _ = canvas.sprite(
            crate::ui_runtime::oreui_assets::PROFILE_GAMERSCORE,
            [
                x - canvas.r(2.4),
                y + canvas.r(2.6),
                x - canvas.r(0.4),
                y + canvas.r(4.6),
            ],
            [255; 4],
        )?;
    }
    Ok(y + canvas.r(7.2))
}

/// Renders only returned statistics, preserving an empty successful list as empty.
pub(super) fn statistics(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
) -> Result<f32, UiPresentationError> {
    if !view.feeds.profile.statistics_loaded {
        super::loading(canvas, b)?;
        return Ok(b[1] + canvas.r(9.2));
    }
    if view.feeds.profile.statistics_error {
        return Ok(b[1]);
    }
    let Some(stats) = &view.feeds.profile.statistics else {
        // A failed statistics lookup is terminal, not a never-ending spinner.
        return Ok(b[1]);
    };
    let values = [
        ("Time played", stats.minutes_played.as_deref()),
        ("Blocks broken", stats.blocks_broken.as_deref()),
        ("Mobs defeated", stats.mobs_defeated.as_deref()),
        ("Distance traveled", stats.distance_travelled.as_deref()),
    ];
    let mut y = b[1];
    for (index, (label, raw)) in values.into_iter().enumerate() {
        let Some(raw) = raw else {
            continue;
        };
        let display = if index == 0 {
            launcher::menu::profile_minutes_display(raw)
        } else {
            launcher::menu::profile_count_display(raw)
        };
        let Some(display) = display else {
            continue;
        };
        let bounds = [b[0], y, b[2], y + canvas.r(7.2)];
        surface(canvas, view, bounds, None)?;
        let icon = crate::ui_runtime::oreui_assets::PROFILE_STAT_ICONS[index];
        let _ = canvas.sprite(
            icon,
            [
                b[0] + canvas.r(0.8),
                y + canvas.r(2.4),
                b[0] + canvas.r(3.2),
                y + canvas.r(4.8),
            ],
            [255; 4],
        )?;
        let text_x = b[0] + canvas.r(4.0);
        canvas.text(
            label,
            [text_x, y + canvas.r(1.6)],
            b[2] - text_x - canvas.r(0.8),
            CAPTION,
            TEXT_DIMMEST,
            false,
        )?;
        canvas.text(
            &display,
            [text_x, y + canvas.r(3.6)],
            b[2] - text_x - canvas.r(0.8),
            CAPTION,
            TEXT,
            false,
        )?;
        y += canvas.r(7.0);
    }
    Ok(if y == b[1] { y } else { y + canvas.r(0.2) })
}

/// Draws a summary label above its count without inventing missing values.
fn summary(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    label: &str,
    value: Option<&str>,
    action: Option<MenuAction>,
) -> Result<(), UiPresentationError> {
    surface(canvas, view, b, action)?;
    let icon = match label {
        "Friends" => 0,
        "Followers" => 1,
        "Screenshot gallery" => 2,
        _ => 3,
    };
    let _ = canvas.sprite(
        crate::ui_runtime::oreui_assets::PROFILE_SUMMARY_ICONS[icon],
        [
            b[0] + canvas.r(0.8),
            b[1] + canvas.r(2.4),
            b[0] + canvas.r(3.2),
            b[1] + canvas.r(4.8),
        ],
        [255; 4],
    )?;
    let x = b[0] + canvas.r(4.0);
    let width = b[2] - x - canvas.r(0.8);
    canvas.text(
        label,
        [x, b[1] + canvas.r(1.6)],
        width,
        CAPTION,
        TEXT_DIMMEST,
        false,
    )?;
    if let Some(value) = value {
        canvas.text(value, [x, b[1] + canvas.r(3.6)], width, BODY, TEXT, false)?;
    }
    Ok(())
}

/// The contiguous neutral summary surface, with semantic hover, press and focus states.
fn surface(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    action: Option<MenuAction>,
) -> Result<(), UiPresentationError> {
    let interaction = Interaction::of(view, action);
    canvas.fill(
        b,
        if interaction.pressed {
            NEUTRAL.pressed
        } else if interaction.hovered {
            NEUTRAL.hovered
        } else {
            NEUTRAL.fill
        },
    )?;
    canvas.frame(b, 0.2, BORDER)?;
    if interaction.focused {
        canvas.frame(b, 0.2, TEXT)?;
    }
    if let Some(action) = action {
        canvas.hit(action, b)?;
    }
    Ok(())
}
