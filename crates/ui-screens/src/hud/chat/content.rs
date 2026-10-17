//! Message definitions resolve independently of the protected chat shell.

use json_ui::{Catalog, ControlScope, RawControl};

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
