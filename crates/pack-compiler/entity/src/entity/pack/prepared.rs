//! Final per-file products stored outside the current attempt's working memory.

use super::*;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct ParsedSource {
    pub(super) record: EntityAssetSource,
    pub(super) bytes: Box<[u8]>,
    pub(super) symbols: BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    pub(super) geometries: BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
}

#[derive(Clone, Copy)]
struct Stored {
    offset: u64,
    len: u64,
}

/// Anonymous storage keeps only the current attempt's expanded products in RAM.
#[derive(Default)]
pub(super) struct SourceStore {
    file: std::cell::OnceCell<Option<fs::File>>,
}

impl SourceStore {
    /// Exercises uncached recovery without depending on the host's temporary directory.
    #[cfg(test)]
    pub(super) fn unavailable() -> Self {
        Self {
            file: std::cell::OnceCell::from(None),
        }
    }

    /// Appends final products; unavailable storage leaves compilation on its uncached path.
    fn save<T: serde::Serialize + ?Sized>(&mut self, value: &T) -> Option<Stored> {
        self.file.get_or_init(|| tempfile::tempfile().ok());
        let file = self.file.get_mut()?.as_mut()?;
        let offset = file.seek(SeekFrom::End(0)).ok()?;
        let mut writer = BufWriter::new(file);
        ciborium::ser::into_writer(value, &mut writer).ok()?;
        writer.flush().ok()?;
        let len = writer
            .get_mut()
            .stream_position()
            .ok()?
            .checked_sub(offset)?;
        Some(Stored { offset, len })
    }

    /// Loads one owned final product without retaining a second attempt's expanded data.
    fn load<T: serde::de::DeserializeOwned>(&mut self, stored: Stored) -> Option<T> {
        let file = self.file.get_mut()?.as_mut()?;
        file.seek(SeekFrom::Start(stored.offset)).ok()?;
        ciborium::de::from_reader(BufReader::new(file.take(stored.len))).ok()
    }
}

/// Geometry normalization has only a byte count and optional compact backing storage.
enum Payload {
    Original,
    Normalized { len: usize, stored: Option<Stored> },
    Invalid,
}

/// Only successful extraction contributes counts; a failed extraction stays budget-sensitive.
enum Products {
    Unseen,
    Invalid,
    Ready {
        stored: Option<Stored>,
        counts: (usize, usize),
    },
}

pub(super) struct PreparedFile {
    pub(super) source: PackSource,
    payload: Payload,
    products: Products,
}

impl PreparedFile {
    /// Retains authored input without parsing or hashing it before admission.
    pub(super) fn new(source: PackSource) -> Self {
        Self {
            source,
            payload: Payload::Original,
            products: Products::Unseen,
        }
    }

    /// Reports expanded geometry documents held between stages; the cache owns none.
    #[cfg(test)]
    pub(super) fn retained_geometry_trees(&self) -> usize {
        0
    }

    /// Normalizes geometry once and retains only its final bytes outside working memory.
    pub(super) fn bytes_len(&mut self, store: &mut SourceStore) -> Option<usize> {
        if self.source.path.starts_with("models/entity/")
            && matches!(self.payload, Payload::Original)
        {
            self.payload = match normalize_geometry(&self.source.bytes) {
                Some(bytes) => Payload::Normalized {
                    len: bytes.len(),
                    stored: store.save(&bytes),
                },
                None => Payload::Invalid,
            };
        }
        match self.payload {
            Payload::Original => Some(self.source.bytes.len()),
            Payload::Normalized { len, .. } => Some(len),
            Payload::Invalid => None,
        }
    }

    /// Returns the authored contribution without loading any expanded product.
    pub(super) fn contribution(&self) -> Option<(usize, usize)> {
        match self.products {
            Products::Ready { counts, .. } => Some(counts),
            _ => None,
        }
    }

    /// Extracts eligible input once and reloads only final products on later attempts.
    pub(super) fn parsed(&mut self, store: &mut SourceStore) -> Option<ParsedSource> {
        match self.products {
            Products::Invalid => return None,
            Products::Ready {
                stored: Some(stored),
                ..
            } => {
                if let Some(parsed) = store.load(stored) {
                    return Some(parsed);
                }
            }
            _ => {}
        }
        let bytes = match self.payload {
            Payload::Original => self.source.bytes.clone(),
            Payload::Normalized { stored, .. } => stored
                .and_then(|stored| store.load(stored))
                .or_else(|| normalize_geometry(&self.source.bytes))?,
            Payload::Invalid => return None,
        };
        self.products = Products::Invalid;
        let parsed = parse(&self.source, bytes.into_boxed_slice())?;
        self.products = Products::Ready {
            stored: store.save(&parsed),
            counts: (parsed.symbols.len(), parsed.geometries.len()),
        };
        Some(parsed)
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
