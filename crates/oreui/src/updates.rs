//! The home notice uses the existing button styles; in-world menus only show a badge.
use super::{
    paint::{Bounds, Canvas},
    theme::{CAPTION, TEXT},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuView, updates::Action};
use ui_engine::engine::screen::PaintError;

/// Draws the two consent actions beside one short verified-release title.
pub(super) fn notice(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
) -> Result<(), PaintError> {
    let Some(ready) = &view.updates.ready else {
        return Ok(());
    };
    if view.over_world {
        return Ok(());
    }
    let [left, top, right, bottom] = bounds;
    canvas.text_line(&ready.title(), [left, top], right - left, CAPTION, TEXT)?;
    let gap = canvas.r(0.8);
    let middle = (left + right - gap) * 0.5;
    let button_top = top + canvas.r(CAPTION.line + 1.2);
    let label = if ready.writable {
        "Restart and update"
    } else {
        "Download update"
    };
    widgets::button(
        canvas,
        view,
        [left, button_top, middle, bottom],
        Variant::Secondary,
        label,
        Some(MenuAction::Update(ready.action())),
    )?;
    widgets::button(
        canvas,
        view,
        [middle + gap, button_top, right, bottom],
        Variant::Neutral,
        "Later",
        Some(MenuAction::Update(Action::Later)),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_tests::paint;
    use std::collections::HashMap;

    #[test]
    fn read_only_install_offers_download_and_world_menus_offer_no_update_actions() {
        let mut view = MenuView::new(true, "Player".into());
        view.updates.ready = Some(launcher::menu::updates::Ready {
            version: "2.0.0".into(),
            writable: false,
            protocol_required: true,
        });
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            notice(canvas, &view, [0.0, 0.0, 500.0, 120.0]).unwrap();
        });
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::Update(Action::Download))
        );
        assert!(
            !hits
                .iter()
                .any(|(action, _)| *action == MenuAction::Update(Action::Restart))
        );
        view.over_world = true;
        let (_, hits, _) = paint(HashMap::new(), |canvas| {
            super::super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
            notice(canvas, &view, [0.0, 0.0, 500.0, 120.0]).unwrap();
        });
        assert!(
            !hits
                .iter()
                .any(|(action, _)| matches!(action, MenuAction::Update(_)))
        );
    }
}
