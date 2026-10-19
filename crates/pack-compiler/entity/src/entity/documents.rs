//! Parsed documents shared only for the lifetime of one assembly attempt.

use super::{SourcePayloads, invalid};
use assets::AssetError;
use pack_compile_input::json::parse_unique_json;
use serde_json::Value;
use std::{cell::OnceCell, collections::BTreeMap, path::Path};

pub(super) struct SourceDocuments<'a> {
    entries: BTreeMap<&'a str, (&'a [u8], OnceCell<Value>)>,
}

impl<'a> SourceDocuments<'a> {
    /// Borrows admitted payloads without parsing or retaining rejected sources.
    pub(super) fn new(payloads: &'a SourcePayloads) -> Self {
        Self {
            entries: payloads
                .iter()
                .map(|(path, bytes)| (path.as_ref(), (bytes.as_ref(), OnceCell::new())))
                .collect(),
        }
    }

    /// Parses a source at its first use and lends the same document to later consumers.
    pub(super) fn read(&self, root: &Path, path: &str) -> Result<&Value, AssetError> {
        let (bytes, document) = self
            .entries
            .get(path)
            .ok_or_else(|| invalid("retained entity source payload is absent"))?;
        if document.get().is_none() {
            #[cfg(test)]
            super::DOCUMENT_PARSES.with_borrow_mut(|counts| {
                *counts.entry(path.to_owned()).or_default() += 1;
            });
            let value = parse_unique_json(&root.join(path), bytes)?;
            document
                .set(value)
                .expect("assembly parses documents sequentially");
        }
        Ok(document.get().unwrap())
    }

    /// Visits admitted paths in their existing sorted order.
    pub(super) fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().copied()
    }
}
