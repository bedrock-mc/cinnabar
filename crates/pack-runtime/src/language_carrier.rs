//! Selects and loads the optional UI language overlay used by runtime packs.
use assets::RuntimeLangCatalog;
use std::{fs::File, io::Read, path::Path, sync::Arc};
const MAX_LANG_ASSET_BLOB_BYTES: u64 = assets::MAX_LANG_CARRIER_BYTES as u64;
/// Selects the UI language for server packs too, and loads its optional table.
pub fn load_active_language(
    world_asset_path: &Path,
    requested: Option<&str>,
) -> Option<Arc<RuntimeLangCatalog>> {
    let code = active_language(requested);
    crate::pack_language::set_active_language(&code);
    load_optional_language(world_asset_path, &code, assets::VANILLA_SOURCE_MANIFEST)
}

/// The UI language: `requested`, else the environment locale, else `en_US`.
#[must_use]
pub fn active_language(requested: Option<&str>) -> String {
    let from_env = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .and_then(|value| locale_code(&value));
    requested
        .map(str::to_owned)
        .or(from_env)
        .unwrap_or_else(|| "en_US".to_owned())
}

/// `de_DE.UTF-8`, `de-de` or `de_DE@euro` as `de_DE`; `None` for `C`/`POSIX`.
fn locale_code(locale: &str) -> Option<String> {
    let base = locale.split(['.', '@']).next()?;
    let (language, country) = base.split_once(['_', '-'])?;
    let code = format!(
        "{}_{}",
        language.to_ascii_lowercase(),
        country.to_ascii_uppercase()
    );
    assets::is_language_code(&code).then_some(code)
}

/// The optional carrier of `code` from `make language-assets`, layered over
/// en_US; `None` (logged) when absent or invalid, so en_US text stands.
#[must_use]
fn load_optional_language(
    world_asset_path: &Path,
    code: &str,
    vanilla_source_json: &str,
) -> Option<Arc<RuntimeLangCatalog>> {
    if code == "en_US" {
        return None;
    }
    let path = world_asset_path
        .with_file_name("lang")
        .join(format!("{code}.mcbelang"));
    let mut bytes = Vec::new();
    let read = File::open(&path).and_then(|file| {
        file.take(MAX_LANG_ASSET_BLOB_BYTES + 1)
            .read_to_end(&mut bytes)
    });
    let loaded = match read {
        Ok(_) if bytes.len() as u64 <= MAX_LANG_ASSET_BLOB_BYTES => {
            RuntimeLangCatalog::decode(&bytes).ok().filter(|catalog| {
                catalog.source_manifest_sha256()
                    == assets::canonical_source_manifest_sha256(vanilla_source_json.as_bytes())
            })
        }
        _ => None,
    };
    match &loaded {
        Some(catalog) => eprintln!(
            "loaded {code} localization from {} ({} entries)",
            path.display(),
            catalog.len()
        ),
        None => eprintln!(
            "{code} localization unavailable at {}; showing en_US text (build it with `make language-assets`)",
            path.display()
        ),
    }
    loaded.map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_strings_reduce_to_language_codes() {
        assert_eq!(locale_code("de_DE.UTF-8").as_deref(), Some("de_DE"));
        assert_eq!(locale_code("pt-br").as_deref(), Some("pt_BR"));
        assert_eq!(locale_code("fr_FR@euro").as_deref(), Some("fr_FR"));
        assert_eq!(locale_code("C"), None);
        assert_eq!(active_language(Some("ja_JP")), "ja_JP");
    }
}
