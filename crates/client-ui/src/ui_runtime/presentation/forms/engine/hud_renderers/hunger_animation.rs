use std::{collections::HashMap, sync::Arc};

use json_ui::Catalog;

/// Update history belongs to each custom control in the current session and pack catalog.
#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct HungerAnimation {
    session: Option<u64>,
    catalog: Option<Arc<Catalog>>,
    updates: HashMap<String, u64>,
}

impl HungerAnimation {
    /// A new session or catalog creates new native renderer instances.
    pub(in crate::ui_runtime::presentation) fn begin(
        &mut self,
        session: u64,
        catalog: &Arc<Catalog>,
    ) {
        if self.session != Some(session)
            || self
                .catalog
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, catalog))
        {
            self.updates.clear();
            self.session = Some(session);
            self.catalog = Some(Arc::clone(catalog));
        }
    }

    /// Advance only the custom control that survived binding and paint visibility gates.
    pub(in crate::ui_runtime::presentation) fn advance(&mut self, key: &str) -> u64 {
        let updates = &mut self.updates;
        if let Some(value) = updates.get_mut(key) {
            *value = value.wrapping_add(1);
            return *value;
        }
        updates.insert(key.to_owned(), 1);
        1
    }
}
