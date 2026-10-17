//! A corner update pill opens a compact panel without occupying the Play menu.
use super::{
    paint::{Bounds, Canvas},
    theme::{CAPTION, EDGE, NEUTRAL80, PRIMARY_ROLE, TEXT_DIMMER},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuView, dialogs::Popup};
use ui_engine::engine::screen::PaintError;

const MARGIN: f32 = 1.6;
const PILL_WIDTH: f32 = 18.0;
const PILL_HEIGHT: f32 = 3.6;
const PANEL_WIDTH: f32 = 28.0;
const PANEL_HEIGHT: f32 = 9.6;
const GAP: f32 = 0.8;

/// Fits the corner controls inside both the viewport width and height.
fn scale(rem: f32, size: [f32; 2]) -> f32 {
    (size[0] / (rem * (PANEL_WIDTH + MARGIN * 2.0)))
        .min(size[1] / (rem * (MARGIN * 2.0 + PILL_HEIGHT + GAP + PANEL_HEIGHT)))
        .min(1.0)
}

/// Reserves a top row so the corner pill never overlaps the home logo.
pub(super) fn header_height(rem: f32, size: [f32; 2]) -> f32 {
    rem * scale(rem, size) * (MARGIN + PILL_HEIGHT + GAP)
}

/// Places the update pill against the viewport's safe inset.
fn pill_bounds(canvas: &Canvas<'_>, size: [f32; 2]) -> Bounds {
    let right = size[0] - canvas.r(MARGIN);
    [
        right - canvas.r(PILL_WIDTH),
        canvas.r(MARGIN),
        right,
        canvas.r(MARGIN + PILL_HEIGHT),
    ]
}

/// Offers release details without giving an optional update the main menu's space.
pub(super) fn pill(
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
        let bounds = pill_bounds(canvas, size);
        let label = if ready.protocol_required {
            "Update required"
        } else {
            "Update available"
        };
        widgets::button(
            canvas,
            view,
            bounds,
            Variant::Neutral,
            label,
            Some(MenuAction::OpenUpdateNotice),
        )?;
        canvas.fill(
            [bounds[0], bounds[1], bounds[0] + canvas.r(0.4), bounds[3]],
            PRIMARY_ROLE.fill,
        )
    })
}

/// Draws an anchored release panel; outside clicks close it without dismissing the release.
pub(super) fn popover(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    popup: &Popup,
    size: [f32; 2],
) -> Result<(), PaintError> {
    canvas.with_scale(scale(canvas.rem, size), |canvas| {
        let pill = pill_bounds(canvas, size);
        let right = pill[2];
        let top = pill[3] + canvas.r(GAP);
        let bounds = [
            right - canvas.r(PANEL_WIDTH),
            top,
            right,
            top + canvas.r(PANEL_HEIGHT),
        ];
        canvas.capture_focus = false;
        for outside in [
            [0.0, 0.0, size[0], bounds[1]],
            [0.0, bounds[1], bounds[0], bounds[3]],
            [bounds[2], bounds[1], size[0], bounds[3]],
            [0.0, bounds[3], size[0], size[1]],
        ] {
            canvas.hit(MenuAction::DismissDialog, outside)?;
        }
        canvas.capture_focus = true;
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
        canvas.text_line(&popup.body, [x, top + pad], right - x, CAPTION, TEXT_DIMMER)?;
        let bottom = bounds[3] - pad;
        widgets::button(
            canvas,
            view,
            [x, bottom - canvas.r(4.4), right, bottom],
            Variant::Secondary,
            &popup.button1,
            Some(popup.confirm),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_tests::paint;
    use launcher::menu::{
        MenuDialog,
        updates::{Action, Ready},
    };
    use std::collections::HashMap;

    #[test]
    fn optional_release_only_offers_the_pill_and_never_installs_from_home() {
        let mut view = MenuView::new(true, "Player".into());
        view.updates.ready = Some(Ready {
            version: "2.0.0".into(),
            writable: true,
            protocol_required: false,
        });
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            super::super::home::draw(canvas, &view, [1280.0, 720.0], None, &|_| None).unwrap();
        });
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::OpenUpdateNotice)
        );
        assert!(
            !hits
                .iter()
                .any(|(action, _)| matches!(action, MenuAction::Update(_)))
        );
    }

    #[test]
    fn release_panel_offers_one_explicit_action_and_outside_close() {
        for writable in [false, true] {
            for size in [[1280.0, 720.0], [360.0, 240.0]] {
                let mut view = MenuView::new(true, "Player".into());
                view.dialog = Some(MenuDialog::UpdateNotice);
                view.updates.ready = Some(Ready {
                    version: "2.0.0".into(),
                    writable,
                    protocol_required: true,
                });
                let popup = launcher::menu::updates::popup(view.updates.ready.as_ref());
                let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
                    popover(canvas, &view, &popup, size).unwrap();
                });
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
                assert!(text.replace(' ', "").contains("Version2.0.0"));
                assert!(!text.contains(popup.title.as_str()));
                assert!(!text.contains("Later"));
                let actions: Vec<_> = hits
                    .iter()
                    .filter_map(|(action, bounds)| {
                        assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
                        assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
                        if let MenuAction::Update(action) = action {
                            Some(*action)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(
                    actions,
                    [if writable {
                        Action::Restart
                    } else {
                        Action::Download
                    }]
                );
                assert!(
                    hits.iter()
                        .any(|(action, _)| *action == MenuAction::DismissDialog)
                );
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
            pill(canvas, &view, [1280.0, 720.0]).unwrap();
        });
        assert!(hits.is_empty());
    }
}
