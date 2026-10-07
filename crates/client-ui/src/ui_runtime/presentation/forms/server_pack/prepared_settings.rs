use std::sync::Arc;

use json_ui::{Catalog, Context};

use crate::ui_runtime::scene_stack::ScreenSettingsTable;

/// Screen policies remain valid only for the exact immutable catalog and context.
#[derive(Debug)]
pub struct PreparedScreenSettings {
    catalog: Arc<Catalog>,
    context: Context,
    settings: Arc<ScreenSettingsTable>,
}

impl PreparedScreenSettings {
    pub(super) fn new(catalog: Arc<Catalog>, context: Context) -> Self {
        let settings = Arc::new(ScreenSettingsTable::for_catalog(&catalog, &context));
        Self {
            catalog,
            context,
            settings,
        }
    }

    pub(in crate::ui_runtime::presentation::forms) fn for_inputs(
        &self,
        catalog: &Arc<Catalog>,
        context: &Context,
    ) -> Option<Arc<ScreenSettingsTable>> {
        (Arc::ptr_eq(&self.catalog, catalog) && self.context == *context)
            .then(|| self.settings.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_screen_context_rejects_prepared_policies() {
        let (namespace, name) = json_ui::HUD_SCREEN.split_once('.').unwrap();
        let definition = serde_json::to_vec(&serde_json::json!({
            "namespace":namespace,
            name:{"type":"screen", "absorbs_input":"$capture"}
        }))
        .unwrap();
        let catalog = Arc::new(
            Catalog::from_files([
                ("ui/_global_variables.json", b"{}".as_slice()),
                (
                    "ui/_ui_defs.json",
                    br#"{"ui_defs":["ui/policy.json"]}"#.as_slice(),
                ),
                ("ui/policy.json", definition.as_slice()),
            ])
            .unwrap(),
        );
        let context = Context::empty().with_flag("capture", true);
        let prepared = PreparedScreenSettings::new(catalog.clone(), context.clone());
        assert!(
            prepared
                .for_inputs(&catalog, &context)
                .unwrap()
                .get(json_ui::HUD_SCREEN)
                .unwrap()
                .absorbs_input
        );
        let changed = context.with_flag("capture", false);
        assert!(prepared.for_inputs(&catalog, &changed).is_none());
        assert!(
            !ScreenSettingsTable::for_catalog(&catalog, &changed)
                .get(json_ui::HUD_SCREEN)
                .unwrap()
                .absorbs_input
        );
    }
}
