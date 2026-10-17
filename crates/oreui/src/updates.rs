//! A quiet release card below the home actions; world menus only show a badge.
use super::{
    paint::{Bounds, Canvas},
    theme::{CAPTION, EDGE, HEADER5, NEUTRAL80, PRIMARY_ROLE, TEXT, TEXT_DIMMER},
    widgets::{self, Variant},
};
use launcher::menu::{MenuAction, MenuView, updates::Action};
use ui_engine::engine::screen::PaintError;

/// Draws a release heading and version above explicit update and defer actions.
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
    let surface = canvas.role(NEUTRAL80);
    canvas.fill(bounds, surface.pressed)?;
    canvas.frame(bounds, EDGE, surface.border)?;
    canvas.fill([left, top, left + canvas.r(0.4), bottom], PRIMARY_ROLE.fill)?;
    let pad = canvas.r(1.6);
    let x = left + pad;
    let right = right - pad;
    let title = if ready.protocol_required {
        "Update required to connect"
    } else {
        "Update available"
    };
    canvas.text_line(title, [x, top + pad], right - x, HEADER5, TEXT)?;
    canvas.text_line(
        &format!("Version {}", ready.version),
        [x, top + pad + canvas.r(HEADER5.line + 0.4)],
        right - x,
        CAPTION,
        TEXT_DIMMER,
    )?;
    let gap = canvas.r(0.8);
    let later_width = canvas.r(8.0);
    let button_top = bottom - pad - canvas.r(4.8);
    let label = if ready.writable {
        "Restart and update"
    } else {
        "Download update"
    };
    widgets::button(
        canvas,
        view,
        [x, button_top, right - later_width - gap, bottom - pad],
        Variant::Secondary,
        label,
        Some(MenuAction::Update(ready.action())),
    )?;
    widgets::button(
        canvas,
        view,
        [right - later_width, button_top, right, bottom - pad],
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
