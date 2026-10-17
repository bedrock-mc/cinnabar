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

    /// Reads the installed pack's native language names without duplicating its list,
    /// falling back to the canonical Minecraft language set when unbundled.
    pub fn language_choices(resource_root: &Path) -> Arc<[(String, String)]> {
        let path = resource_root
            .join(crate::install_layout::vanilla_pack_relative())
            .join("texts/language_names.json");
        let mut bytes = Vec::new();
        if let Ok(mut file) = File::open(path) {
            if file
                .take(MAX_LANGUAGE_NAMES_BYTES + 1)
                .read_to_end(&mut bytes)
                .is_ok()
                && bytes.len() as u64 <= MAX_LANGUAGE_NAMES_BYTES
            {
                let choices = parse_choices(&bytes);
                if !choices.is_empty() {
                    return choices.into();
                }
            }
        }
        parse_choices(DEFAULT_LANGUAGE_NAMES_JSON.as_bytes()).into()
    }
}

/// Canonical official Minecraft Bedrock languages fallback when raw pack texts are not unpacked.
pub const DEFAULT_LANGUAGE_NAMES_JSON: &str = r#"[
  ["en_US", "English (United States)"],
  ["en_GB", "English (United Kingdom)"],
  ["de_DE", "Deutsch (Deutschland)"],
  ["es_ES", "Español (España)"],
  ["es_MX", "Español (México)"],
  ["fr_FR", "Français (France)"],
  ["fr_CA", "Français (Canada)"],
  ["it_IT", "Italiano (Italia)"],
  ["ja_JP", "日本語 (日本)"],
  ["ko_KR", "한국어 (대한민국)"],
  ["pt_BR", "Português (Brasil)"],
  ["pt_PT", "Português (Portugal)"],
  ["ru_RU", "Русский (Россия)"],
  ["zh_CN", "中文(简体)"],
  ["zh_TW", "中文(繁體)"],
  ["nl_NL", "Nederlands (Nederland)"],
  ["bg_BG", "Български (България)"],
  ["cs_CZ", "Čeština (Česko)"],
  ["da_DK", "Dansk (Danmark)"],
  ["el_GR", "Ελληνικά (Ελλάда)"],
  ["fi_FI", "Suomi (Suomi)"],
  ["hu_HU", "Magyar (Magyarország)"],
  ["id_ID", "Indonesia (Indonesia)"],
  ["nb_NO", "Norsk bokmål (Norge)"],
  ["pl_PL", "Polski (Polska)"],
  ["sk_SK", "Slovenčina (Slovensko)"],
  ["sv_SE", "Svenska (Sverige)"],
  ["tr_TR", "Türkçe (Türkiye)"],
  ["uk_UA", "Українська (Україна)"]
]"#;

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
    fn language_choices_fall_back_to_canonical_minecraft_languages() {
        let choices = SettingsOptions::language_choices(Path::new("/nonexistent"));
        assert_eq!(choices.len(), 29);
        assert!(choices.iter().any(|(code, name)| code == "fr_FR" && name.contains("Français")));
        assert!(choices.iter().any(|(code, name)| code == "de_DE" && name.contains("Deutsch")));
        assert!(choices.iter().any(|(code, name)| code == "zh_CN" && name.contains("中文")));
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
