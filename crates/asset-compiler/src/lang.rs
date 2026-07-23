//! Localization compiler: reduces the pinned pack's `texts/en_US.lang` into
//! the bounded sorted carrier consumed by chat/rawtext resolution and item
//! display names.

use std::{collections::BTreeMap, fs, path::Path};

use assets::{
    LangEntry, MAX_LANG_ENTRIES, MAX_LANG_KEY_BYTES, MAX_LANG_VALUE_BYTES, encode_lang_catalog,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::entity::validate_vanilla_source_manifest;

const LANG_RELATIVE_PATH: &str = "texts/en_US.lang";
const MAX_LANG_SOURCE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct CompiledLangCarrier {
    pub bytes: Vec<u8>,
    pub report: LangCompileReport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LangCompileReport {
    pub source_manifest_sha256: [u8; 32],
    pub lang_source_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub entries: usize,
    pub duplicate_keys: usize,
    pub skipped_oversized: usize,
    pub source_bytes: usize,
}

#[derive(Debug, Error)]
pub enum LangCompileError {
    #[error("language source manifest is not the reviewed identity: {0}")]
    SourceManifest(#[from] assets::AssetError),
    #[error("language source {path} could not be read: {source}")]
    SourceRead {
        path: Box<Path>,
        #[source]
        source: std::io::Error,
    },
    #[error("language source {path} exceeds the {maximum}-byte bound")]
    SourceTooLarge { path: Box<Path>, maximum: usize },
    #[error("language source {path} is not UTF-8")]
    SourceNotUtf8 { path: Box<Path> },
    #[error(
        "language source {path} is not the pinned official Mojang sample `texts/en_US.lang` (sha256 {actual}, pinned {expected})"
    )]
    SourceBytesMismatch {
        path: Box<Path>,
        actual: String,
        expected: String,
    },
    #[error("language table exceeds the {maximum}-entry bound")]
    TooManyEntries { maximum: usize },
    #[error("language carrier encoding failed: {0}")]
    Carrier(#[from] assets::LangCatalogError),
}

/// Compiles the carrier from `<root>/texts/en_US.lang`. When
/// `expected_source_sha256` is set (production always pins
/// [`assets::VANILLA_EN_US_LANG_SHA256`]), source bytes with any other
/// identity are refused before an entry is parsed.
pub fn compile_lang_assets(
    root: &Path,
    source_manifest: &[u8],
    expected_source_sha256: Option<[u8; 32]>,
) -> Result<CompiledLangCarrier, LangCompileError> {
    let source_manifest_sha256 = validate_vanilla_source_manifest(source_manifest)?;
    let path = root.join(LANG_RELATIVE_PATH);
    let bytes = fs::read(&path).map_err(|source| LangCompileError::SourceRead {
        path: path.clone().into_boxed_path(),
        source,
    })?;
    if bytes.len() > MAX_LANG_SOURCE_BYTES {
        return Err(LangCompileError::SourceTooLarge {
            path: path.into_boxed_path(),
            maximum: MAX_LANG_SOURCE_BYTES,
        });
    }
    let lang_source_sha256: [u8; 32] = Sha256::digest(&bytes).into();
    if let Some(expected) = expected_source_sha256
        && lang_source_sha256 != expected
    {
        return Err(LangCompileError::SourceBytesMismatch {
            path: path.into_boxed_path(),
            actual: hex_lower(&lang_source_sha256),
            expected: hex_lower(&expected),
        });
    }
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).map_err(
        |_| LangCompileError::SourceNotUtf8 {
            path: path.into_boxed_path(),
        },
    )?;

    let mut table = BTreeMap::<Box<str>, Box<str>>::new();
    let mut duplicate_keys = 0usize;
    let mut skipped_oversized = 0usize;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with("##") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        // Bedrock language values may carry a trailing tab-hash comment.
        let value = value
            .split_once("\t#")
            .map_or(value, |(cleaned, _)| cleaned);
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        if key.len() > MAX_LANG_KEY_BYTES || value.len() > MAX_LANG_VALUE_BYTES {
            skipped_oversized += 1;
            continue;
        }
        // Later lines override earlier ones, matching the vanilla loader.
        if table.insert(key.into(), value.into()).is_some() {
            duplicate_keys += 1;
        }
    }
    if table.len() > MAX_LANG_ENTRIES {
        return Err(LangCompileError::TooManyEntries {
            maximum: MAX_LANG_ENTRIES,
        });
    }
    let entries: Vec<LangEntry> = table
        .into_iter()
        .map(|(key, value)| LangEntry {
            key,
            value: value.into(),
        })
        .collect();
    let carrier = encode_lang_catalog(source_manifest_sha256, lang_source_sha256, &entries)?;
    Ok(CompiledLangCarrier {
        report: LangCompileReport {
            source_manifest_sha256,
            lang_source_sha256,
            carrier_sha256: Sha256::digest(&carrier).into(),
            entries: entries.len(),
            duplicate_keys,
            skipped_oversized,
            source_bytes: bytes.len(),
        },
        bytes: carrier,
    })
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lang_lines_parse_with_comments_overrides_and_bounds() {
        let root = std::env::temp_dir().join(format!(
            "cinnabar-lang-compile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("texts")).unwrap();
        let oversized_value = "x".repeat(MAX_LANG_VALUE_BYTES + 1);
        let source = format!(
            "## comment line\r\nitem.apple.name=Apple\t#with comment\r\n\r\nitem.apple.name=Apple Override\r\ncommands.op.success=Opped: %s\r\nbroken line without equals\r\ntoo.big={oversized_value}\r\n"
        );
        fs::write(root.join(LANG_RELATIVE_PATH), source).unwrap();
        let manifest = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("assets/vanilla-source.json"),
        )
        .unwrap();

        let compiled = compile_lang_assets(&root, &manifest, None).unwrap();
        assert_eq!(compiled.report.entries, 2);
        assert_eq!(compiled.report.duplicate_keys, 1);
        assert_eq!(compiled.report.skipped_oversized, 1);

        let catalog = assets::RuntimeLangCatalog::decode(&compiled.bytes).unwrap();
        assert_eq!(
            catalog.lookup("item.apple.name").as_deref(),
            Some("Apple Override")
        );
        assert_eq!(
            catalog.lookup("commands.op.success").as_deref(),
            Some("Opped: %s")
        );
        assert_eq!(catalog.lookup("missing"), None);
        // The exact source-byte identity is embedded in the carrier.
        let source_bytes = fs::read(root.join(LANG_RELATIVE_PATH)).unwrap();
        let expected: [u8; 32] = Sha256::digest(&source_bytes).into();
        assert_eq!(catalog.lang_source_sha256(), expected);
        assert_eq!(compiled.report.lang_source_sha256, expected);

        // With the pinned production identity enforced, tampered source
        // bytes beside the canonical manifest refuse to compile.
        let refused =
            compile_lang_assets(&root, &manifest, Some(assets::VANILLA_EN_US_LANG_SHA256));
        assert!(matches!(
            refused,
            Err(LangCompileError::SourceBytesMismatch { .. })
        ));

        fs::remove_dir_all(root).unwrap();
    }
}
