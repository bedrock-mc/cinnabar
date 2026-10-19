//! Per-file work retained across isolation attempts.

use super::*;

pub(super) enum Skip {
    Oversized,
    Unparsable,
}

pub(super) struct ParsedSource {
    pub(super) record: EntityAssetSource,
    pub(super) bytes: Box<[u8]>,
    pub(super) symbols: BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    pub(super) geometries: BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    pub(super) valid: bool,
}

pub(super) struct PreparedFile {
    pub(super) path: Box<str>,
    pub(super) parsed: Result<ParsedSource, Skip>,
}

impl PreparedFile {
    /// Retains normalization, extracted definitions and the digest for every later attempt.
    pub(super) fn new(mut source: PackSource) -> Self {
        Self {
            parsed: prepare(&mut source),
            path: source.path,
        }
    }
}

/// Parses a source independently; admission and its byte budget are decided at merge time.
fn prepare(source: &mut PackSource) -> Result<ParsedSource, Skip> {
    let mut geometry = None;
    let bytes = if source.path.starts_with("models/entity/") {
        let mut value =
            pack_json::parse_jsonc::<Value>(&source.bytes).map_err(|_| Skip::Unparsable)?;
        if !sanitize::sanitize_geometry(&mut value) {
            return Err(Skip::Unparsable);
        }
        let bytes = serde_json::to_vec(&value).map_err(|_| Skip::Unparsable)?;
        geometry = Some(value);
        bytes.into_boxed_slice()
    } else {
        std::mem::take(&mut source.bytes).into_boxed_slice()
    };
    if bytes.len() > assets::MAX_ENTITY_SOURCE_BYTES {
        return Err(Skip::Oversized);
    }
    let mut symbols = BTreeMap::new();
    let mut geometries = BTreeMap::new();
    #[cfg(test)]
    SOURCE_PARSES.update(|count| count + 1);
    let result = if let Some(value) = geometry {
        parse_geometry(
            &source.path,
            Path::new(source.authored_path.as_ref()),
            &value,
            &mut symbols,
            &mut geometries,
        )
    } else {
        source.parse(&bytes, &mut symbols, &mut geometries)
    };
    let valid = result.is_ok() && file_identifiers_are_valid(&symbols, &geometries);
    if !valid {
        symbols.clear();
        geometries.clear();
    }
    Ok(ParsedSource {
        record: EntityAssetSource {
            path: source.path.clone(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(&bytes).into(),
        },
        bytes,
        symbols,
        geometries,
        valid,
    })
}
