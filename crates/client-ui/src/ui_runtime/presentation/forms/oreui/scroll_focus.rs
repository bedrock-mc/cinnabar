//! Focus reveals a control once, preserving later wheel movement.

use super::paint::Canvas;
use crate::ui_runtime::presentation::{menu_scroll::MenuScrolls, rect};
use launcher::menu::{MenuAction, MenuView};

pub(super) fn reveal(canvas: &mut Canvas<'_>, scrolls: &mut MenuScrolls, view: &MenuView) -> bool {
    let target = view
        .focused_action
        .and_then(|action| canvas.focus_hits.iter().find(|(found, _)| *found == action));
    let Some((action, bounds)) = target else {
        scrolls.observe_focus(view.focused_action);
        return false;
    };
    let fixed = *action == MenuAction::AddBack
        || (view.screen == launcher::menu::MenuScreen::AddServer
            && matches!(action, MenuAction::AddSave | MenuAction::AddSaveConnect))
        || (*action == MenuAction::SettingsScalePicker && view.settings_scale_picker)
        || matches!(action, MenuAction::SettingsDropdown(index) if view.settings_dropdown == Some(*index));
    let centre = (bounds.min().x() + bounds.max().x()) * 0.5;
    let area = canvas
        .scrolls
        .iter()
        .find(|area| centre >= area.viewport.min().x() && centre <= area.viewport.max().x());
    let Some(area) = area.filter(|_| !fixed) else {
        scrolls.observe_focus(view.focused_action);
        return false;
    };
    let min = bounds.min();
    let max = bounds.max();
    let Ok(unscrolled) = rect(
        min.x(),
        min.y() + area.offset,
        max.x(),
        max.y() + area.offset,
    ) else {
        return false;
    };
    let offset = scrolls.reveal_focus(
        &area.key,
        Some(*action),
        Some(unscrolled),
        area.viewport,
        area.max,
    );
    if (offset - area.offset).abs() <= f32::EPSILON {
        return false;
    }
    canvas.offsets.insert(area.key.clone(), offset);
    true
}
