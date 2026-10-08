//! The vanilla language list supplies its native names and ordering at runtime.

use std::{fs::File, io::Read, path::Path, sync::Arc};

use super::SettingsOptions;

const MAX_LANGUAGE_NAMES_BYTES: u64 = 64 * 1024;

impl SettingsOptions {
    /// An absent choice follows the startup locale.
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// Accepts locale codes only; an unchanged choice needs no save or reload.
    pub fn set_language(&mut self, code: &str) -> bool {
        if !assets::is_language_code(code) || self.language() == Some(code) {
            return false;
        }
        self.language = Some(code.to_owned());
        true
    }

    /// Reads the installed pack's native language names without duplicating its list.
    pub fn language_choices(resource_root: &Path) -> Arc<[(String, String)]> {
        let path = resource_root
            .join(crate::install_layout::vanilla_pack_relative())
            .join("texts/language_names.json");
        let mut bytes = Vec::new();
        let Ok(file) = File::open(path) else {
            return Arc::from([]);
        };
        if file
            .take(MAX_LANGUAGE_NAMES_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_LANGUAGE_NAMES_BYTES
        {
            return Arc::from([]);
        }
        parse_choices(&bytes).into()
    }
}

/// Drops malformed locale rows before their names reach the UI.
fn parse_choices(bytes: &[u8]) -> Vec<(String, String)> {
    serde_json::from_slice::<Vec<(String, String)>>(bytes)
        .unwrap_or_default()
        .into_iter()
        .filter(|(code, name)| assets::is_language_code(code) && !name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_language_order_and_names_come_from_the_pack() {
        assert_eq!(
            parse_choices(br#"[["de_DE","Deutsch"],["en_US","English"],["bad","Bad"]]"#),
            vec![
                ("de_DE".into(), "Deutsch".into()),
                ("en_US".into(), "English".into())
            ]
        );
    }

    #[test]
    fn language_default_and_persistence_round_trip() {
        let mut settings = SettingsOptions::default();
        assert_eq!(settings.language(), None);
        assert!(!settings.set_language("../bad"));
        assert!(settings.set_language("de_DE"));
        let bytes = serde_json::to_vec(&settings).unwrap();
        assert_eq!(
            SettingsOptions::decode(&bytes).unwrap().language(),
            Some("de_DE")
        );
    }
}
