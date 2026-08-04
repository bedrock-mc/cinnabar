use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use assets::{
    LanguageCatalogError, MAX_LANGUAGE_TOTAL_BYTES, RuntimeLanguageCatalog,
    encode_language_catalog, parse_language_bytes,
};
use sha2::{Digest, Sha256};
use thiserror::Error;

const LANGUAGE_FILE: &str = "texts/en_US.lang";

pub struct CompiledLanguageCarrier {
    pub bytes: Box<[u8]>,
    pub report: LanguageCompileReport,
}

pub struct LanguageCompileReport {
    pub schema: u32,
    pub source_manifest_sha256: [u8; 32],
    pub carrier_sha256: [u8; 32],
    pub entries: usize,
    pub source_bytes: usize,
}

#[derive(Debug, Error)]
pub enum LanguageCompileError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} exceeds the {limit}-byte language source bound")]
    TooLarge { path: PathBuf, limit: usize },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: LanguageCatalogError,
    },
    #[error("could not encode the language carrier: {0}")]
    Encode(#[from] LanguageCatalogError),
}

pub fn compile_language_assets(
    pack: &Path,
    source_manifest_sha256: [u8; 32],
) -> Result<CompiledLanguageCarrier, LanguageCompileError> {
    let path = pack.join(LANGUAGE_FILE);
    let file = File::open(&path).map_err(|source| LanguageCompileError::Read {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(MAX_LANGUAGE_TOTAL_BYTES).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| LanguageCompileError::Read {
            path: path.clone(),
            source,
        })?;
    if bytes.len() > MAX_LANGUAGE_TOTAL_BYTES {
        return Err(LanguageCompileError::TooLarge {
            path,
            limit: MAX_LANGUAGE_TOTAL_BYTES,
        });
    }
    let translations =
        parse_language_bytes(&bytes).map_err(|source| LanguageCompileError::Parse {
            path: path.clone(),
            source,
        })?;
    let carrier = encode_language_catalog(source_manifest_sha256, &translations)?;
    let carrier_sha256 = Sha256::digest(&carrier).into();
    let runtime = RuntimeLanguageCatalog::decode(&carrier, source_manifest_sha256)?;
    Ok(CompiledLanguageCarrier {
        bytes: carrier,
        report: LanguageCompileReport {
            schema: runtime.identity().schema,
            source_manifest_sha256,
            carrier_sha256,
            entries: translations.len(),
            source_bytes: bytes.len(),
        },
    })
}
