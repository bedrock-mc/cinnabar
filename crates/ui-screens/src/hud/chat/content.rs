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
