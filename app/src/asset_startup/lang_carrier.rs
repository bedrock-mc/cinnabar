//! Required pinned localization carrier loading, split from the asset-startup
//! root to honor the production line budget.
//!
//! Chat translation keys and item display names are player-facing text, so
//! production startup requires this carrier exactly like the HUD carrier: a
//! missing, oversized, malformed, or stale-provenance carrier is a fatal,
//! actionable error naming the rebuild command.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::RuntimeLangCatalog;

use super::{
    AssetStartupError, DEFAULT_ASSET_PATH, canonical_source_manifest_sha256, format_sha256,
};

pub const LANG_ASSETS_FILENAME: &str = assets::carriers::LANG.output;
pub const LANG_ASSETS_COMPILE_COMMAND: &str = "make lang-assets";
const LANG_ASSETS_REPORT_FILENAME: &str = "lang-assets.json";
const MAX_LANG_ASSET_BLOB_BYTES: u64 = 8 * 1024 * 1024;

/// Returns a copy-paste recovery command that writes the localization
/// carrier where startup looked for it: the bare make target at the default
/// location, or the same target with `LANG_ASSET_BLOB`/`LANG_ASSET_REPORT`
/// naming the exact custom siblings.
#[must_use]
pub fn lang_assets_rebuild_command(path: &Path) -> String {
    let default_path = lang_asset_path(Path::new(DEFAULT_ASSET_PATH));
    if path == default_path {
        return LANG_ASSETS_COMPILE_COMMAND.to_owned();
    }
    let report_path = path.with_file_name(LANG_ASSETS_REPORT_FILENAME);
    format!(
        "{LANG_ASSETS_COMPILE_COMMAND} LANG_ASSET_BLOB={} LANG_ASSET_REPORT={}",
        super::shell_quote_path(path),
        super::shell_quote_path(&report_path)
    )
}

#[derive(Debug)]
pub struct LoadedLangAssets {
    runtime: Arc<RuntimeLangCatalog>,
    selected_path: PathBuf,
}

impl LoadedLangAssets {
    #[must_use]
    pub fn runtime(&self) -> &Arc<RuntimeLangCatalog> {
        &self.runtime
    }

    pub fn into_runtime(self) -> Arc<RuntimeLangCatalog> {
        self.runtime
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "loaded pinned official Mojang sample localization from {} ({} entries, source_manifest_sha256={}, lang_source_sha256={})",
            self.selected_path.display(),
            self.runtime.len(),
            format_sha256(self.runtime.source_manifest_sha256()),
            format_sha256(self.runtime.lang_source_sha256())
        )
    }
}

/// Selects the UI language for server packs too, and loads its optional table.
pub fn load_active_language(
    world_asset_path: &Path,
    requested: Option<&str>,
) -> Option<Arc<RuntimeLangCatalog>> {
    let code = active_language(requested);
    crate::runtime::network::set_active_language(&code);
    load_optional_language(
        world_asset_path,
        &code,
        super::vanilla_source_manifest_json(),
    )
}

/// The UI language: `requested`, else the environment locale, else `en_US`.
#[must_use]
pub(crate) fn active_language(requested: Option<&str>) -> String {
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
                    == canonical_source_manifest_sha256(vanilla_source_json)
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

#[must_use]
pub fn lang_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(LANG_ASSETS_FILENAME)
}

/// Loads and validates the localization carrier beside `world_asset_path`,
/// failing closed on absence, size, decode, or stale provenance against the
/// embedded canonical `vanilla-source.json` identity.
pub fn require_lang_assets(
    world_asset_path: &Path,
    vanilla_source_json: &str,
) -> Result<LoadedLangAssets, AssetStartupError> {
    let path = lang_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            let rebuild_command = lang_assets_rebuild_command(&path);
            return Err(AssetStartupError::LangAssetsMissing {
                notice: format!(
                    "required pinned official Mojang sample localization carrier was not found at {}; chat translation and item names cannot present, so the client will not start. Build it with `{rebuild_command}`, or refresh every required carrier with `make assets`.",
                    path.display()
                ),
                rebuild_command,
                path,
            });
        }
        Err(source) => {
            return Err(AssetStartupError::LangAssetsRead {
                rebuild_command: lang_assets_rebuild_command(&path),
                path,
                source,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::LangAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: lang_assets_rebuild_command(&path),
        })?
        .len();
    if length > MAX_LANG_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::LangAssetsTooLarge {
            rebuild_command: lang_assets_rebuild_command(&path),
            path,
            max_bytes: MAX_LANG_ASSET_BLOB_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_LANG_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::LangAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: lang_assets_rebuild_command(&path),
        })?;
    if bytes.len() as u64 > MAX_LANG_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::LangAssetsTooLarge {
            rebuild_command: lang_assets_rebuild_command(&path),
            path,
            max_bytes: MAX_LANG_ASSET_BLOB_BYTES,
        });
    }
    let runtime = RuntimeLangCatalog::decode(&bytes).map_err(|source| {
        AssetStartupError::LangAssetsDecode {
            path: path.clone(),
            source,
            rebuild_command: lang_assets_rebuild_command(&path),
        }
    })?;
    let expected = canonical_source_manifest_sha256(vanilla_source_json);
    if runtime.source_manifest_sha256() != expected {
        return Err(AssetStartupError::LangAssetsProvenance {
            rebuild_command: lang_assets_rebuild_command(&path),
            carrier: format_sha256(runtime.source_manifest_sha256()),
            manifest: format_sha256(expected),
            path,
        });
    }
    // The carrier must also have been compiled from the exact pinned
    // `texts/en_US.lang` bytes: a tampered language file beside the
    // canonical manifest fails closed here.
    if runtime.lang_source_sha256() != assets::VANILLA_EN_US_LANG_SHA256 {
        return Err(AssetStartupError::LangAssetsSourceProvenance {
            rebuild_command: lang_assets_rebuild_command(&path),
            carrier: format_sha256(runtime.lang_source_sha256()),
            pinned: format_sha256(assets::VANILLA_EN_US_LANG_SHA256),
            path,
        });
    }
    Ok(LoadedLangAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    })
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
