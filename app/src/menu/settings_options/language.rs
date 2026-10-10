//! Host handoff for selected language assets.
use crate::menu::MenuRuntime;
use client_ui::ui_runtime::UiRuntime;
use std::{path::PathBuf, sync::Arc};

impl MenuRuntime {
    /// Uses the selected carrier path and gives a CLI locale priority over the saved choice.
    pub(crate) fn with_language_assets(mut self, path: PathBuf, requested: Option<&str>) -> Self {
        self.language_asset_path = path;
        let code =
            crate::asset_startup::active_language(requested.or(self.settings_options.language()));
        Arc::make_mut(&mut self.settings_options).set_language(&code);
        self.language_pending = true;
        self
    }

    /// Changes the language selected by the vanilla radio collection.
    pub(in crate::menu) fn set_language(&mut self, index: u16) {
        let Some((code, _)) =
            self.language_choices
                .get(launcher::menu::settings_options::language::usize::from(
                    index,
                ))
        else {
            return;
        };
        if Arc::make_mut(&mut self.settings_options).set_language(code) {
            self.settings_dirty = true;
            self.language_pending = true;
        }
    }

    /// Installs a selected carrier once; missing optional translations fall back to English.
    pub(crate) fn sync_language(&mut self, runtime: &mut UiRuntime) {
        if !self.language_pending {
            return;
        }
        self.language_pending = false;
        let catalog = crate::asset_startup::load_active_language(
            &self.language_asset_path,
            self.settings_options.language(),
        );
        runtime.set_active_language(catalog);
    }
}

#[cfg(test)]
mod tests {
    use launcher::menu::settings_options::language::*;
    #[test]
    fn selecting_a_language_updates_runtime_text_and_english_restores_the_base() {
        let directory =
            std::env::temp_dir().join(format!("cinnabar-language-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("lang")).unwrap();
        let provenance = crate::asset_startup::canonical_source_manifest_sha256(
            crate::asset_startup::vanilla_source_manifest_json(),
        );
        let entries = [assets::LangEntry {
            key: "tile.stone.name".into(),
            value: "Stein".into(),
        }];
        let bytes = assets::encode_lang_catalog(provenance, [0; 32], &entries).unwrap();
        std::fs::write(directory.join("lang/de_DE.mcbelang"), bytes).unwrap();
        let mut menu = MenuRuntime::new(true, 2, "Steve".into())
            .with_language_assets(directory.join("world.mcbea"), Some("en_US"));
        menu.language_choices = Arc::from([
            ("en_US".to_owned(), "English".to_owned()),
            ("de_DE".to_owned(), "Deutsch".to_owned()),
        ]);
        let entries = [assets::LangEntry {
            key: "tile.stone.name".into(),
            value: "Stone".into(),
        }];
        let bytes = assets::encode_lang_catalog(provenance, [0; 32], &entries).unwrap();
        let mut runtime = UiRuntime::new(0);
        runtime.set_lang_catalog(Arc::new(
            assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
        ));
        menu.set_language(1);
        menu.sync_language(&mut runtime);
        assert_eq!(runtime.localized_item_name("minecraft:stone"), "Stein");
        menu.set_language(0);
        menu.sync_language(&mut runtime);
        assert_eq!(runtime.localized_item_name("minecraft:stone"), "Stone");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
