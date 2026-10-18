//! A compact corner card keeps the update action outside the Play menu.
use super::{
    paint::{Bounds, Canvas},
    theme::{CAPTION, EDGE, NEUTRAL80, PRIMARY_ROLE, TEXT_DIMMER},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuView};
use ui_engine::engine::screen::PaintError;

const MARGIN: f32 = 1.6;
const WIDTH: f32 = 28.0;
const HEIGHT: f32 = 9.6;

/// Fits the complete card and its inset inside the viewport.
fn scale(rem: f32, size: [f32; 2]) -> f32 {
    (size[0] / (rem * (WIDTH + MARGIN * 2.0)))
        .min(size[1] / (rem * (HEIGHT + MARGIN * 2.0)))
        .min(1.0)
}

/// Leaves room above the logo when a narrow viewport puts the card over it.
pub(super) fn header_height(rem: f32, size: [f32; 2], logo_width: f32) -> f32 {
    let fitted_rem = rem * scale(rem, size);
    let card_left = size[0] - fitted_rem * (WIDTH + MARGIN);
    let logo_right = (size[0] + logo_width) * 0.5;
    if card_left >= logo_right {
        0.0
    } else {
        fitted_rem * (MARGIN + HEIGHT + 0.8)
    }
}

/// Shows the release version and one explicit update action directly in the corner.
pub(super) fn notice(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), PaintError> {
    let Some(ready) = &view.updates.ready else {
        return Ok(());
    };
    if view.over_world {
        return Ok(());
    }
    canvas.with_scale(scale(canvas.rem, size), |canvas| {
        let right = size[0] - canvas.r(MARGIN);
        let top = canvas.r(MARGIN);
        let bounds: Bounds = [right - canvas.r(WIDTH), top, right, top + canvas.r(HEIGHT)];
        let surface = canvas.role(NEUTRAL80);
        canvas.fill(bounds, surface.pressed)?;
        canvas.frame(bounds, EDGE, surface.border)?;
        canvas.fill(
            [bounds[0], top, bounds[0] + canvas.r(0.4), bounds[3]],
            PRIMARY_ROLE.fill,
        )?;
        let pad = canvas.r(1.2);
        let x = bounds[0] + pad;
        let right = right - pad;
        let status = if ready.protocol_required {
            "Update required"
        } else {
            "Update available"
        };
        canvas.text_line(
            &format!("{status} · {}", ready.version),
            [x, top + pad],
            right - x,
            CAPTION,
            TEXT_DIMMER,
        )?;
        let bottom = bounds[3] - pad;
        let label = if ready.writable {
            "Restart and update"
        } else {
            "Download update"
        };
        widgets::button(
            canvas,
            view,
            [x, bottom - canvas.r(4.4), right, bottom],
            Variant::Secondary,
            label,
            Some(MenuAction::Update(ready.action())),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_tests::paint;
    use launcher::menu::updates::{Action, Ready};
    use std::collections::HashMap;

    #[test]
    fn corner_card_directly_offers_one_update_button_without_later_or_an_extra_opener() {
        for writable in [false, true] {
            for required in [false, true] {
                for size in [[1280.0, 720.0], [360.0, 240.0]] {
                    let mut view = MenuView::new(true, "Player".into());
                    view.updates.ready = Some(Ready {
                        version: "2.0.0".into(),
                        writable,
                        protocol_required: required,
                    });
                    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
                        notice(canvas, &view, size).unwrap();
                    });
                    assert_eq!(hits.len(), 1);
                    assert_eq!(
                        hits[0].0,
                        MenuAction::Update(if writable {
                            Action::Restart
                        } else {
                            Action::Download
                        })
                    );
                    let bounds = hits[0].1;
                    assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
                    assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
                    let text: String = nodes
                        .iter()
                        .filter_map(|node| match node.visual() {
                            ui::UiVisual::Text { layout, .. } => Some(
                                layout
                                    .glyphs()
                                    .iter()
                                    .map(|glyph| glyph.codepoint)
                                    .collect::<String>(),
                            ),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(text.replace(' ', "").matches("Update").count(), 1);
                    assert!(!text.contains("Later"));
                }
            }
        }
    }

    #[test]
    fn world_menus_offer_no_update_controls() {
        let mut view = MenuView::new(true, "Player".into());
        view.over_world = true;
        view.updates.ready = Some(Ready {
            version: "2.0.0".into(),
            writable: true,
            protocol_required: true,
        });
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            notice(canvas, &view, [1280.0, 720.0]).unwrap();
        });
        assert!(hits.is_empty());
    }
}
