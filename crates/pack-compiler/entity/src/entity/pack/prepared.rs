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
    #[cfg(test)]
    retained_documents: Option<std::sync::Arc<memory::RetainedDocuments>>,
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
            #[cfg(test)]
            retained_documents: None,
        }
    }

    /// Caches final products once admitted, retaining only length for rejected geometry.
    pub(super) fn parsed(&mut self, remaining: SourceBudget) -> Result<&ParsedSource, Skip> {
        if let Some(length) = self.length {
            check_admission(length, remaining)?;
        }
        if self.products.is_none() {
            let (bytes, document) = if self.source.path.starts_with("models/entity/") {
                let normalized = normalize_geometry(
                    &self.source.bytes,
                    #[cfg(test)]
                    self.retained_documents.clone(),
                );
                self.length = Some(normalized.as_ref().map(|input| input.bytes.len()));
                check_admission(self.length.flatten(), remaining)?;
                let normalized = normalized.unwrap();
                (normalized.bytes, Some(normalized.document))
            } else {
                (self.source.bytes.clone().into_boxed_slice(), None)
            };
            self.products = Some(parse(&self.source, bytes, document));
        }
        self.products
            .as_ref()
            .and_then(Option::as_ref)
            .ok_or(Skip::Unparsable)
    }
}

/// Extracts independently only when every possible admitted size fits the remaining budget.
/// Large input batches and tight budgets stay lazy and ordered; parser trees never queue here.
pub(super) fn prepare_batch(files: &mut [PreparedFile], remaining: SourceBudget) {
    let bytes = files
        .iter()
        .map(|file| match file.length {
            Some(length) => length.unwrap_or(0),
            None => assets::MAX_ENTITY_SOURCE_BYTES,
        })
        .sum::<usize>();
    let input_bytes = files
        .iter()
        .map(|file| file.source.bytes.len())
        .sum::<usize>();
    if files.len() > remaining.count
        || bytes > remaining.bytes
        || input_bytes > assets::MAX_ENTITY_SOURCE_BYTES
    {
        return;
    }
    #[cfg(test)]
    let before = files.iter().filter(|file| file.products.is_some()).count();
    files.par_iter_mut().for_each(|file| {
        let _ = file.parsed(remaining);
    });
    #[cfg(test)]
    SOURCE_PARSES.update(|count| {
        count + files.iter().filter(|file| file.products.is_some()).count() - before
    });
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

/// A geometry document held only until ordered admission and extraction finish.
struct NormalizedGeometry {
    bytes: Box<[u8]>,
    document: Value,
    #[cfg(test)]
    _retained: Option<memory::RetainedDocument>,
}

/// Normalizes a geometry document once for its payload and extraction.
fn normalize_geometry(
    bytes: &[u8],
    #[cfg(test)] retained: Option<std::sync::Arc<memory::RetainedDocuments>>,
) -> Option<NormalizedGeometry> {
    let mut value = pack_json::parse_jsonc::<Value>(bytes).ok()?;
    if !sanitize::sanitize_geometry(&mut value) {
        return None;
    }
    Some(NormalizedGeometry {
        bytes: serde_json::to_vec(&value).ok()?.into_boxed_slice(),
        document: value,
        #[cfg(test)]
        _retained: retained.map(memory::RetainedDocument::new),
    })
}

/// Extracts final products only after admission, releasing all parser intermediates locally.
fn parse(source: &PackSource, bytes: Box<[u8]>, document: Option<Value>) -> Option<ParsedSource> {
    let mut symbols = BTreeMap::new();
    let mut geometries = BTreeMap::new();
    if let Some(document) = document {
        parse_geometry(
            &source.path,
            Path::new(source.authored_path.as_ref()),
            &document,
            &mut symbols,
            &mut geometries,
        )
        .ok()?;
    } else {
        source.parse(&bytes, &mut symbols, &mut geometries).ok()?;
    }
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

#[cfg(test)]
mod memory;
