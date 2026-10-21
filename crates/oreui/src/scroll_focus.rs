//! Focus reveals a control once, preserving later wheel movement.

use super::paint::Canvas;
use launcher::menu::{MenuAction, MenuView};
use launcher::shader_options::Action as ShaderOptionsAction;
use ui_engine::{engine::rect, menu_scroll::MenuScrolls};

pub(super) fn reveal(
    canvas: &mut Canvas<'_>,
    scrolls: &mut MenuScrolls<MenuAction>,
    view: &MenuView,
) -> bool {
    let target = view
        .focused_action
        .and_then(|action| canvas.focus_hits.iter().find(|(found, _)| *found == action));
    let Some((action, bounds)) = target else {
        scrolls.observe_focus(view.focused_action);
        return false;
    };
    // Controls outside the scroll view whose columns they share, such as the Shader Pack
    // Settings footer: revealing one would scroll that view to its end.
    let fixed = *action == MenuAction::AddBack
        || (view.screen == launcher::menu::MenuScreen::AddServer
            && matches!(action, MenuAction::AddSave | MenuAction::AddSaveConnect))
        || (*action == MenuAction::SettingsScalePicker && view.settings.scale_picker)
        || matches!(action, MenuAction::SettingsDropdown(index) if view.settings.dropdown == Some(*index))
        || matches!(
            action,
            MenuAction::ShaderOptions(
                ShaderOptionsAction::Back
                    | ShaderOptionsAction::Reset
                    | ShaderOptionsAction::Cancel
                    | ShaderOptionsAction::Apply
                    | ShaderOptionsAction::Done
            )
        );
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
