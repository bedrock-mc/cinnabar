//! Message definitions resolve independently of the protected chat shell.

use json_ui::{Catalog, ControlScope, RawControl};

use super::Rules;

/// Snapshots pack definitions before suppression and gives retained message roots
/// that scope. Shared template overrides then affect content without changing the shell.
pub(super) fn isolate(
    catalog: &mut Catalog,
    row: &mut Option<RawControl>,
    messages: &mut Option<RawControl>,
) -> Option<ControlScope> {
    if row.is_none() && messages.is_none() {
        return None;
    }
    let scope = catalog.scope_definitions();
    for control in row.iter_mut().chain(messages.iter_mut()) {
        scope.apply(control);
    }
    Some(scope)
}

/// Keeps filters from replaced placements on a row retaining the built-in child hierarchy.
/// Authored hierarchies and the shared snapshot keep their individual visibility rules.
pub(super) fn retain_fallback_filters(row: &mut RawControl, builtin: &RawControl, rules: &Rules) {
    if same_hierarchy(row, builtin) {
        rules.merge_into(row);
    }
}

/// Ensures resource-pack overrides of `hud.chat_grid_item` still honor the player's
/// configured `$chat_font_scale_factor`, `$chat_line_spacing`, and `$chat_background_opacity`
/// on the closed HUD chat instead of hardcoding fixed scale or alpha values.
pub(super) fn preserve_chat_settings_vars(control: &mut RawControl) {
    if control.name == "chat_background" {
        control.props.insert(
            "alpha".to_owned(),
            serde_json::Value::String("$chat_background_opacity".to_owned()),
        );
    }
    if control.name == "chat_text"
        || control.name == "chat_label"
        || control
            .base
            .as_deref()
            .is_some_and(|base| base.ends_with("chat_label"))
    {
        control.props.insert(
            "font_scale_factor".to_owned(),
            serde_json::Value::String("$chat_font_scale_factor".to_owned()),
        );
        control.props.insert(
            "line_padding".to_owned(),
            serde_json::Value::String("($chat_line_spacing - 1)".to_owned()),
        );
    }
    for child in &mut control.children {
        preserve_chat_settings_vars(child);
    }
}

/// Whether control names, inheritance and child placement still follow the built-in row.
fn same_hierarchy(control: &RawControl, builtin: &RawControl) -> bool {
    control.name == builtin.name
        && control.base == builtin.base
        && control.has_controls == builtin.has_controls
        && control.children.len() == builtin.children.len()
        && control
            .children
            .iter()
            .zip(&builtin.children)
            .all(|(child, original)| same_hierarchy(child, original))
}
