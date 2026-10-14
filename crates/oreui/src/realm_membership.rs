//! Invitation entry, read-only verification, confirmation and accepted membership.

use launcher::menu::realm_membership::{Action, Stage};

use super::{
    grid::Grid,
    modal,
    paint::Canvas,
    theme::{CAPTION, EDGE, HEADER_HEIGHT, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuView};
use ui_engine::engine::screen::PaintError;

const PAD: f32 = 1.6;
const FIELD_HEIGHT: f32 = 4.8;
const BUTTON_HEIGHT: f32 = 4.4;
const LABEL_GAP: f32 = 0.8;
const INVITE_HELP: &str = "Ask the Realm owner for an invite link or code.";

/// Resolves the invite form's content span at the current geometry and text scale.
fn form_span(canvas: &Canvas<'_>, width: f32) -> [f32; 2] {
    let grid = Grid::new(canvas.r(1.0), width);
    grid.span(
        if grid.narrow { 0 } else { 1 },
        if grid.narrow { 8 } else { 10 },
    )
}

/// Draws the invitation flow with input isolated from the underlying Play tabs.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), PaintError> {
    let Some(state) = &view.realm_membership else {
        return Ok(());
    };
    let scale = if state.stage == Stage::Code {
        let [left, right] = form_span(canvas, size[0]);
        let error = match state.error {
            Some(error) => canvas.measure_height(error, right - left, CAPTION)? + canvas.r(PAD),
            None => 0.0,
        };
        let minimum = canvas.r(HEADER_HEIGHT
            + EDGE
            + PAD * 4.0
            + CAPTION.line
            + LABEL_GAP
            + FIELD_HEIGHT
            + BUTTON_HEIGHT)
            + canvas.measure_height(INVITE_HELP, right - left, CAPTION)?
            + error;
        (size[1] / minimum).min(1.0)
    } else {
        1.0
    };
    canvas.with_scale(scale, |canvas| draw_fitted(canvas, view, size))
}

/// Paints a complete invite form at the fitted scale; other stages keep their modal layout.
fn draw_fitted(canvas: &mut Canvas<'_>, view: &MenuView, size: [f32; 2]) -> Result<(), PaintError> {
    let Some(state) = &view.realm_membership else {
        return Ok(());
    };
    let action = |action| MenuAction::RealmMembership(action);
    widgets::screen_overlay(canvas, size)?;
    let top = widgets::header(
        canvas,
        view,
        "Join Realm",
        size[0],
        state.can_back().then_some(action(Action::Back)),
    )?;
    let [left, right] = form_span(canvas, size[0]);
    let pad = canvas.r(PAD);
    let mut y = top + pad;
    if let Some(error) = state.error {
        y += canvas.text(
            error,
            [left, y],
            right - left,
            CAPTION,
            super::theme::DESTRUCTIVE_TINT,
            false,
        )? + pad;
    }
    match state.stage {
        Stage::Code => {
            canvas.text_line(
                "Invite link or code",
                [left, y],
                right - left,
                CAPTION,
                TEXT,
            )?;
            y += canvas.r(CAPTION.line + LABEL_GAP);
            let field = [left, y, right, y + canvas.r(FIELD_HEIGHT)];
            widgets::text_field(
                canvas,
                view,
                field,
                &state.code,
                "Enter an invite link or code",
                view.field == action(Action::EditCode).text_field(),
                Some(action(Action::EditCode)),
            )?;
            y = field[3] + pad;
            y += canvas.text(
                INVITE_HELP,
                [left, y],
                right - left,
                CAPTION,
                TEXT_DIMMER,
                false,
            )? + pad;
            widgets::button(
                canvas,
                view,
                [left, y, right, y + canvas.r(BUTTON_HEIGHT)],
                Variant::Primary,
                "Join",
                state.can_verify().then_some(action(Action::Verify)),
            )?;
        }
        Stage::Verifying | Stage::Joining => {
            let title = if state.stage == Stage::Verifying {
                "Verifying invite link..."
            } else {
                "Joining Realm..."
            };
            let prompt = modal::Modal {
                title,
                items: Vec::new(),
                body: "This may take a few moments.".into(),
                body_color: TEXT,
                buttons: if state.can_back() {
                    vec![(
                        "Cancel".into(),
                        Variant::Secondary,
                        Some(action(Action::Back)),
                    )]
                } else {
                    Vec::new()
                },
                close: state.can_back().then_some(action(Action::Back)),
            };
            canvas.clear_focus_geometry();
            modal::draw(canvas, view, size, &prompt)?;
        }
        Stage::Confirm | Stage::Complete => {
            let Some(realm) = &state.realm else {
                return Ok(());
            };
            let accepted = state.stage == Stage::Complete;
            let body = if accepted && !state.can_play() {
                format!(
                    "You joined {}. This Realm is currently unavailable.",
                    realm.name
                )
            } else if accepted {
                format!(
                    "You joined {}. You can play now or return to your Realms.",
                    realm.name
                )
            } else if realm.owner.is_empty() {
                format!("Would you like to join {}?", realm.name)
            } else {
                format!(
                    "Would you like to join {}?\nOwner: {}",
                    realm.name, realm.owner
                )
            };
            let body = if let Some(error) = state.error {
                format!("{body}\n\n{error}")
            } else {
                body
            };
            let prompt = modal::Modal {
                title: if accepted {
                    "Realm joined"
                } else {
                    "Join Realm"
                },
                items: Vec::new(),
                body: body.into(),
                body_color: TEXT,
                buttons: vec![
                    (
                        (if accepted { "Play" } else { "Join Realm" }).into(),
                        Variant::Primary,
                        (!accepted || state.can_play()).then_some(action(if accepted {
                            Action::Play
                        } else {
                            Action::Accept
                        })),
                    ),
                    (
                        (if accepted { "Done" } else { "Back" }).into(),
                        Variant::Secondary,
                        Some(action(Action::Back)),
                    ),
                ],
                close: Some(action(Action::Back)),
            };
            canvas.clear_focus_geometry();
            modal::draw(canvas, view, size, &prompt)?;
        }
    }
    Ok(())
}
