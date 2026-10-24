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

/// Whether the home layout must reserve space for an app or mod update notice.
pub(super) fn available(view: &MenuView) -> bool {
    view.updates.manual_pending
        || view.updates.ready.is_some()
        || !view.mods.library.updates.is_empty()
}

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

/// Shows one app or mod update notice with an explicit action in the home corner.
pub(super) fn notice(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), PaintError> {
    let (status, label, action) = if view.updates.manual_pending {
        (
            format!("Close other {} windows", launcher::PRODUCT_NAME),
            "Install pending",
            None,
        )
    } else if let Some(ready) = &view.updates.ready {
        let status = if ready.protocol_required {
            "Update required"
        } else {
            "Update available"
        };
        (
            format!("{status} · {}", ready.version),
            if ready.writable {
                "Restart and update"
            } else {
                "Download update"
            },
            Some(MenuAction::Update(ready.action())),
        )
    } else {
        let updates = &view.mods.library.updates;
        let Some(update) = updates.first() else {
            return Ok(());
        };
        let title = match updates.len() {
            1 => view
                .mods
                .library
                .installed
                .iter()
                .find(|item| item.id == update.id)
                .map(|item| format!("Mod update · {}", item.name()))
                .unwrap_or_else(|| "Mod update available".into()),
            count => format!("Mod updates available · {count}"),
        };
        (
            title,
            "View mod updates",
            Some(MenuAction::Mods(launcher::mods::Action::ViewUpdates)),
        )
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
        canvas.text_line(&status, [x, top + pad], right - x, CAPTION, TEXT_DIMMER)?;
        let bottom = bounds[3] - pad;
        widgets::button(
            canvas,
            view,
            [x, bottom - canvas.r(4.4), right, bottom],
            Variant::Secondary,
            label,
            action,
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
    fn a_manual_copy_waiting_for_the_original_client_shows_no_install_action() {
        for size in [[1280.0, 720.0], [360.0, 240.0]] {
            let mut view = MenuView::new(true, "Player".into());
            view.updates.active = false;
            view.updates.manual_pending = true;
            assert!(available(&view));
            let (_, hits, _) = paint(HashMap::new(), |canvas| {
                notice(canvas, &view, size).unwrap();
            });
            assert!(hits.is_empty());
        }
    }

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

    #[test]
    fn mod_notices_use_the_corner_card_and_open_the_mods_tab() {
        use launcher::mods::{Action as ModAction, AvailableUpdate};
        for size in [[1280.0, 720.0], [360.0, 240.0]] {
            let mut view = MenuView::new(true, "Player".into());
            std::sync::Arc::make_mut(&mut view.mods)
                .library
                .updates
                .push(AvailableUpdate {
                    id: 1,
                    version: "1.1.0".into(),
                    release_url: String::new(),
                });
            let (_, hits, _) = paint(HashMap::new(), |canvas| {
                notice(canvas, &view, size).unwrap();
            });
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].0, MenuAction::Mods(ModAction::ViewUpdates));
            let bounds = hits[0].1;
            assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
            assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
            view.updates.ready = Some(Ready {
                version: "2.0.0".into(),
                writable: true,
                protocol_required: false,
            });
            let (_, hits, _) = paint(HashMap::new(), |canvas| {
                notice(canvas, &view, size).unwrap();
            });
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].0, MenuAction::Update(Action::Restart));
            view.over_world = true;
            let (_, hits, _) = paint(HashMap::new(), |canvas| {
                notice(canvas, &view, size).unwrap();
            });
            assert!(hits.is_empty());
        }
    }
}
