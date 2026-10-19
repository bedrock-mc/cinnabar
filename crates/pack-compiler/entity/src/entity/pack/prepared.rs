//! Per-file work retained across isolation attempts.

use super::*;

pub(super) struct ParsedSource {
    pub(super) record: EntityAssetSource,
    pub(super) symbols: BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    pub(super) geometries: BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
}

pub(super) struct PreparedFile {
    pub(super) source: PackSource,
    normalized: Option<bool>,
    geometry: Option<Value>,
    pub(super) parsed: std::cell::OnceCell<Option<ParsedSource>>,
}

impl PreparedFile {
    /// Retains authored input without parsing or hashing it before admission.
    pub(super) fn new(source: PackSource) -> Self {
        Self {
            source,
            normalized: None,
            geometry: None,
            parsed: std::cell::OnceCell::new(),
        }
    }

    /// Normalizes geometry once before returning the byte count used for admission.
    pub(super) fn bytes_len(&mut self) -> Option<usize> {
        if self.source.path.starts_with("models/entity/") && self.normalized.is_none() {
            let geometry = normalize_geometry(&self.source.bytes);
            self.normalized = Some(geometry.is_some());
            if let Some((bytes, value)) = geometry {
                self.source.bytes = bytes;
                // An oversized document cannot be admitted on a later attempt.
                self.geometry =
                    (self.source.bytes.len() <= assets::MAX_ENTITY_SOURCE_BYTES).then_some(value);
            }
        }
        (self.normalized != Some(false)).then_some(self.source.bytes.len())
    }

    /// Extracts and hashes admitted input once, caching malformed definitions as failures.
    pub(super) fn parsed(&mut self) -> Option<&ParsedSource> {
        let geometry = self.geometry.take();
        self.parsed
            .get_or_init(|| {
                let mut symbols = BTreeMap::new();
                let mut geometries = BTreeMap::new();
                #[cfg(test)]
                SOURCE_PARSES.update(|count| count + 1);
                if let Some(value) = geometry {
                    parse_geometry(
                        &self.source.path,
                        Path::new(self.source.authored_path.as_ref()),
                        &value,
                        &mut symbols,
                        &mut geometries,
                    )
                } else {
                    self.source
                        .parse(&self.source.bytes, &mut symbols, &mut geometries)
                }
                .ok()?;
                if !file_identifiers_are_valid(&symbols, &geometries) {
                    return None;
                }
                Some(ParsedSource {
                    record: EntityAssetSource {
                        path: self.source.path.clone(),
                        source_bytes: self.source.bytes.len() as u32,
                        source_sha256: Sha256::digest(&self.source.bytes).into(),
                    },
                    symbols,
                    geometries,
                })
            })
            .as_ref()
    }
}

/// Returns the sanitized payload and parsed value without parsing the serialized bytes again.
fn normalize_geometry(bytes: &[u8]) -> Option<(Vec<u8>, Value)> {
    let mut value = pack_json::parse_jsonc::<Value>(bytes).ok()?;
    if !sanitize::sanitize_geometry(&mut value) {
        return None;
    }
    Some((serde_json::to_vec(&value).ok()?, value))
}
