//! The death screen (`/gameplay/death`): a dark red vignette, "You Died!" and
//! the Respawn (hero) and Game Menu (secondary) buttons in a 32rem column,
//! spaced 3 : 5 : 2 around the title and buttons.

use super::super::super::UiPresentationError;
use super::paint::Canvas;
use super::theme::{BODY, HEADER3, TEXT};
use super::widgets::{Variant, button};
use crate::menu::{MenuAction, MenuScreen, MenuView};

/// The vignette's centre and edge colours (radial in the original).
const CENTRE: [u8; 4] = [0, 0, 0, 102];
const EDGE_COLOUR: [u8; 4] = [45, 4, 4, 204];
const BANDS: usize = 6;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    let [width, height] = size;
    // Nested bands step from the edge colour to the centre colour.
    for band in 0..BANDS {
        let t = band as f32 / (BANDS - 1) as f32;
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        let colour = [
            mix(EDGE_COLOUR[0], CENTRE[0]),
            mix(EDGE_COLOUR[1], CENTRE[1]),
            mix(EDGE_COLOUR[2], CENTRE[2]),
            if band == 0 { EDGE_COLOUR[3] } else { 24 },
        ];
        let inset = t * 0.35;
        canvas.fill(
            [
                width * inset,
                height * inset,
                width * (1.0 - inset),
                height * (1.0 - inset),
            ],
            colour,
        )?;
    }
    let column = canvas.r(32.0).min(width - canvas.r(1.6));
    let left = (width - column) * 0.5;
    let title_height = canvas.r(HEADER3.line);
    let button_height = canvas.r(4.4);
    let gap = canvas.r(0.4);
    let buttons_height = button_height * 2.0 + gap;
    let free = (height - title_height - buttons_height).max(0.0);
    let title_top = free * 0.3;
    canvas.text_centred(
        "You Died!",
        [left, title_top, left + column, title_top + title_height],
        HEADER3,
        TEXT,
        true,
    )?;
    let reason_top = title_top + title_height + canvas.r(1.0);
    let reason_bounds = [
        width * 0.15,
        reason_top,
        width * 0.85,
        reason_top + height * 0.2,
    ];
    let clip = canvas.begin_clip(reason_bounds)?;
    canvas.text_centred(&view.death_reason, reason_bounds, BODY, TEXT, true)?;
    canvas.end_clip(clip);
    let buttons_top = title_top + title_height + free * 0.5;
    button(
        canvas,
        view,
        [
            left,
            buttons_top,
            left + column,
            buttons_top + button_height,
        ],
        Variant::Hero,
        "Respawn",
        Some(MenuAction::Respawn),
    )?;
    let second = buttons_top + button_height + gap;
    button(
        canvas,
        view,
        [left, second, left + column, second + button_height],
        Variant::Secondary,
        "Game Menu",
        Some(MenuAction::Navigate(MenuScreen::Pause)),
    )
}
