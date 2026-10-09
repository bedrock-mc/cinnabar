//! The death overlay keeps the world and HUD beneath independently timed content.

mod backdrop;

use launcher::menu::death::{CONTENT_FADE_SECONDS, STAGE_SECONDS};

use super::super::super::UiPresentationError;
use super::super::menu_screens::Translate;
use super::{paint::Canvas, theme, widgets};
use crate::menu::{MenuAction, MenuView};

/// Samples a CSS cubic Bezier by solving its horizontal parameter first.
fn bezier(t: f64, control: [f64; 4]) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let curve = |s: f64, a: f64, b: f64| {
        3.0 * (1.0 - s).powi(2) * s * a + 3.0 * (1.0 - s) * s * s * b + s.powi(3)
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..24 {
        let s = (low + high) * 0.5;
        if curve(s, control[0], control[2]) < t {
            low = s;
        } else {
            high = s;
        }
    }
    curve((low + high) * 0.5, control[1], control[3]) as f32
}

/// Samples an independently delayed content opacity, respecting disabled animations.
fn fade(age: f64, delay: f64, animations: bool) -> f32 {
    if age < delay {
        return 0.0;
    }
    if !animations {
        return 1.0;
    }
    bezier((age - delay) / CONTENT_FADE_SECONDS, [0.42, 0.0, 0.58, 1.0])
}

/// Keeps the death backdrop beneath the game menu while its content is covered.
pub(super) fn background(
    canvas: &mut Canvas<'_>,
    state: launcher::menu::death::DeathPresentation,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    let age = if state.immediate_respawn {
        state.elapsed_seconds - STAGE_SECONDS
    } else {
        state.elapsed_seconds
    };
    backdrop::draw(canvas, size, age, state.animations)
}

/// Paints the message, staggered actions, and retained respawn progress.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
) -> Result<(), UiPresentationError> {
    let state = view.death_presentation;
    let pending_age = state.respawn_seconds.or(view.death_loading.then_some(1.0));
    background(canvas, state, size)?;
    canvas.bundle = theme::Bundle::Gameplay;
    canvas.appearance = theme::Appearance::Default;
    let content_age = pending_age.map_or(state.elapsed_seconds, |age| state.elapsed_seconds - age);
    let prompt_at = if state.animations { STAGE_SECONDS } else { 0.0 };
    let exit = pending_age.map_or(1.0, |age| 1.0 - fade(age, 0.0, state.animations));
    let content = |delay| {
        if let Some(age) = state.return_seconds {
            fade(age, 0.0, state.animations)
        } else {
            fade(content_age, delay, state.animations)
        }
    };
    let message_alpha = if state.immediate_respawn {
        0.0
    } else {
        content(prompt_at) * exit
    };
    let primary_alpha = if state.immediate_respawn {
        0.0
    } else {
        content(state.controls_at()) * exit
    };
    let secondary_alpha = if state.immediate_respawn {
        0.0
    } else {
        content(
            state.controls_at()
                + if state.animations {
                    CONTENT_FADE_SECONDS
                } else {
                    0.0
                },
        ) * exit
    };
    let width = canvas.r(32.0).min(size[0]);
    let left = (size[0] - width) * 0.5;
    let right = left + width;
    let title_height = canvas.r(theme::HEADER3.line);
    let primary_height = canvas.r(theme::BUTTON_HEIGHT);
    let secondary_height = canvas.r(theme::BUTTON_HEIGHT);
    let gap = canvas.r(1.0);
    let action_height = primary_height + gap + secondary_height + canvas.r(3.0);
    let reason_height = canvas
        .measure_height(&view.death_reason, width, theme::BODY)?
        .min((size[1] - title_height - action_height).max(0.0));
    let free = (size[1] - title_height - reason_height - action_height).max(0.0);
    let message_top = free * 0.3;
    let action_top = message_top + title_height + reason_height + free * 0.5;
    let alpha = canvas.alpha;
    canvas.alpha = alpha * message_alpha;
    let title = if state.hardcore {
        translate("gameplay.DeathScreen.gameOverTitle").unwrap_or_else(|| "GAME OVER!".into())
    } else {
        translate("gameplay.DeathScreen.youDied").unwrap_or_else(|| "YOU DIED!".into())
    };
    canvas.text_centred(
        &title,
        [left, message_top, right, message_top + title_height],
        theme::HEADER3,
        theme::TEXT,
        true,
    )?;
    let reason_clip = canvas.begin_clip([
        left,
        message_top + title_height,
        right,
        message_top + title_height + reason_height,
    ])?;
    canvas.centered_wrapped_text_with_shadow(
        &view.death_reason,
        [left, message_top + title_height],
        width,
        theme::BODY,
        theme::TEXT,
        true,
    )?;
    canvas.end_clip(reason_clip);
    let ready = view.death_controls_visible && state.controls_ready() && !view.death_loading;
    for (label, bounds, variant, action, opacity) in [
        (
            if state.hardcore {
                translate("gameplay.DeathScreen.exitWorldButton")
                    .unwrap_or_else(|| "EXIT WORLD".into())
            } else {
                translate("gameplay.DeathScreen.respawn").unwrap_or_else(|| "RESPAWN".into())
            },
            [left, action_top, right, action_top + primary_height],
            widgets::Variant::Hero,
            if state.hardcore {
                MenuAction::DeathExitWorld
            } else {
                MenuAction::Respawn
            },
            primary_alpha,
        ),
        (
            if state.hardcore {
                translate("gameplay.DeathScreen.spectateWorldButton")
                    .unwrap_or_else(|| "Spectate world".into())
            } else {
                translate("gameplay.DeathScreen.gameMenu").unwrap_or_else(|| "Game menu".into())
            },
            [
                left,
                action_top + primary_height + gap,
                right,
                action_top + primary_height + gap + secondary_height,
            ],
            widgets::Variant::Secondary,
            if state.hardcore {
                MenuAction::Respawn
            } else {
                MenuAction::OpenDeathGameMenu
            },
            secondary_alpha,
        ),
    ] {
        canvas.alpha = alpha * opacity;
        let interaction = canvas.interaction(view, ready.then_some(action));
        widgets::button_face(canvas, bounds, variant, &label, interaction, ready)?;
        if ready {
            canvas.hit(action, bounds)?;
        }
    }
    if let Some(age) = pending_age {
        let delay = if state.exiting_world {
            0.0
        } else {
            STAGE_SECONDS
        };
        canvas.alpha = alpha * fade(age, delay, state.animations);
        let bottom = size[1] - canvas.r(6.4);
        let loader_size = canvas.r(4.8);
        super::progress::loader(
            canvas,
            [
                (size[0] - loader_size) * 0.5,
                bottom - canvas.r(theme::BODY.line + 1.0) - loader_size,
                (size[0] + loader_size) * 0.5,
                bottom - canvas.r(theme::BODY.line + 1.0),
            ],
        )?;
        let label = if state.hardcore {
            translate("gameplay.DeathScreen.savingWorld").unwrap_or_else(|| "Saving world".into())
        } else {
            translate("gameplay.DeathScreen.respawning").unwrap_or_else(|| "Respawning".into())
        };
        canvas.text_centred(
            &label,
            [left, bottom - canvas.r(theme::BODY.line), right, bottom],
            theme::BODY,
            theme::TEXT,
            false,
        )?;
    }
    canvas.alpha = alpha;
    Ok(())
}
