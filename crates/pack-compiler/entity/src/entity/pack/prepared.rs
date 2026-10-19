//! Final per-file products prepared only after source admission.

use super::*;

#[derive(Clone)]
pub(super) struct ParsedSource {
    pub(super) record: EntityAssetSource,
    pub(super) bytes: Box<[u8]>,
    pub(super) symbols: BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    pub(super) geometries: BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
}

pub(super) enum Skip {
    Unparsable,
    Oversized,
    OverBudget,
}

pub(super) struct PreparedFile {
    pub(super) source: PackSource,
    /// Unknown until normalization; an inner None records malformed geometry.
    pub(super) length: Option<Option<usize>>,
    /// Unknown until admission; an inner None records failed extraction.
    pub(super) products: Option<Option<ParsedSource>>,
}

impl PreparedFile {
    /// Retains authored input without parsing or hashing it before admission.
    pub(super) fn new(source: PackSource) -> Self {
        let length =
            (!source.path.starts_with("models/entity/")).then_some(Some(source.bytes.len()));
        Self {
            source,
            length,
            products: None,
        }
    }

    /// Caches final products once admitted, retaining only length for rejected geometry.
    pub(super) fn parsed(&mut self, remaining: SourceBudget) -> Result<&ParsedSource, Skip> {
        if let Some(length) = self.length {
            check_admission(length, remaining)?;
        }
        if self.products.is_none() {
            let bytes = if self.source.path.starts_with("models/entity/") {
                normalize_geometry(&self.source.bytes)
            } else {
                Some(self.source.bytes.clone())
            };
            self.length = Some(bytes.as_ref().map(Vec::len));
            check_admission(self.length.flatten(), remaining)?;
            self.products = Some(parse(&self.source, bytes.unwrap().into_boxed_slice()));
        }
        self.products
            .as_ref()
            .and_then(Option::as_ref)
            .ok_or(Skip::Unparsable)
    }
}

/// Preserves malformed, oversized and admission-budget rejection precedence.
fn check_admission(length: Option<usize>, remaining: SourceBudget) -> Result<(), Skip> {
    let length = length.ok_or(Skip::Unparsable)?;
    if length > assets::MAX_ENTITY_SOURCE_BYTES {
        Err(Skip::Oversized)
    } else if remaining.count == 0 || length > remaining.bytes {
        Err(Skip::OverBudget)
    } else {
        Ok(())
    }
}

/// Normalizes one geometry document and releases its JSON tree before returning.
fn normalize_geometry(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut value = pack_json::parse_jsonc::<Value>(bytes).ok()?;
    if !sanitize::sanitize_geometry(&mut value) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

/// Extracts final products only after admission, releasing all parser intermediates locally.
fn parse(source: &PackSource, bytes: Box<[u8]>) -> Option<ParsedSource> {
    #[cfg(test)]
    SOURCE_PARSES.update(|count| count + 1);
    let mut symbols = BTreeMap::new();
    let mut geometries = BTreeMap::new();
    source.parse(&bytes, &mut symbols, &mut geometries).ok()?;
    if !file_identifiers_are_valid(&symbols, &geometries) {
        return None;
    }
    Some(ParsedSource {
        record: EntityAssetSource {
            path: source.path.clone(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(&bytes).into(),
        },
        bytes,
        symbols,
        geometries,
    })
}
