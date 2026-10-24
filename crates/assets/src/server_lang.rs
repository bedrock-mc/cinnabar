//! Optional session translations, independent of the pinned base carrier's format limits.

use std::{
    collections::{HashMap, TryReserveError},
    sync::Arc,
};

/// Immutable session translations. Clones share the table; lookups borrow its text.
pub struct ServerLangOverlay {
    entries: HashMap<String, String>,
}

impl std::fmt::Debug for ServerLangOverlay {
    /// Reports table size without exposing server-provided strings.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ServerLangOverlay")
            .field("entry_count", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl ServerLangOverlay {
    /// Reads one complete source. The reader must verify its size and integrity.
    /// Allocation or read failure is diagnosed without publishing incomplete input.
    pub fn read(declared_bytes: usize, read: impl FnOnce(&mut [u8]) -> bool) -> Option<Arc<Self>> {
        let mut input = Vec::new();
        if input.try_reserve_exact(declared_bytes).is_err() {
            tracing::warn!(
                reason = "input_allocation",
                source_bytes = declared_bytes,
                "skipping server language input"
            );
            return None;
        }
        input.resize(declared_bytes, 0);
        if !read(&mut input) {
            tracing::warn!(
                reason = "read_failed",
                source_bytes = declared_bytes,
                "skipping server language input"
            );
            return None;
        }
        Self::from_layers([input])
    }

    /// Appends sources in precedence order. Invalid sources and entries that cannot
    /// be allocated are diagnosed and skipped without dropping other definitions.
    pub fn from_layers(sources: impl IntoIterator<Item = impl AsRef<[u8]>>) -> Option<Arc<Self>> {
        let mut entries = HashMap::new();
        let mut readable = false;
        for (source_index, source) in sources.into_iter().enumerate() {
            let bytes = source.as_ref();
            let Ok(text) = std::str::from_utf8(bytes) else {
                tracing::warn!(
                    reason = "invalid_utf8",
                    source_index,
                    source_bytes = bytes.len(),
                    "skipping server language input"
                );
                continue;
            };
            readable = true;
            let text = text.strip_prefix('\u{feff}').unwrap_or(text);
            let mut skipped = 0usize;
            for (key, value) in translation_rows(text) {
                let candidate = (|| {
                    let mut key = copy_text(key)?;
                    key.make_ascii_lowercase();
                    let value = decode_value(value)?;
                    if let Some(previous) = entries.get_mut(&key) {
                        *previous = value;
                    } else {
                        entries.try_reserve(1)?;
                        entries.insert(key, value);
                    }
                    Ok::<_, TryReserveError>(())
                })();
                if candidate.is_err() {
                    skipped += 1;
                }
            }
            if skipped != 0 {
                tracing::warn!(
                    reason = "entry_allocation",
                    source_index,
                    skipped_entries = skipped,
                    "skipping server language definitions"
                );
            }
        }
        if !readable {
            return None;
        }
        Some(Arc::new(Self { entries }))
    }

    /// Finds a definition using the same ASCII case folding as insertion.
    #[must_use]
    pub fn lookup(&self, key: &str) -> Option<&str> {
        if let Some(value) = self.entries.get(key) {
            return Some(value);
        }
        if !key.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return None;
        }
        let mut folded = copy_text(key).ok()?;
        folded.make_ascii_lowercase();
        self.entries.get(&folded).map(String::as_str)
    }
}

/// Copies one translation field only after its allocation succeeds.
fn copy_text(text: &str) -> Result<String, TryReserveError> {
    let mut owned = String::new();
    owned.try_reserve_exact(text.len())?;
    owned.push_str(text);
    Ok(owned)
}

/// Decodes language-file newlines and backslashes once, keeping other escapes literal.
fn decode_value(value: &str) -> Result<String, TryReserveError> {
    if !value.contains('\\') {
        return copy_text(value);
    }
    let mut decoded = String::new();
    decoded.try_reserve_exact(value.len())?;
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => decoded.push('\n'),
                Some('\\') => decoded.push('\\'),
                next => {
                    decoded.push('\\');
                    if let Some(ch) = next {
                        decoded.push(ch);
                    }
                }
            }
        } else {
            decoded.push(ch);
        }
    }
    Ok(decoded)
}

/// Borrows valid translations in source order without copying ignored rows.
fn translation_rows(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(|line| {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with("##") {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        let value = value.split_once("\t#").map_or(value, |(value, _)| value);
        Some((key, value))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_values_decode_newlines_and_backslashes_once() {
        let table = ServerLangOverlay::from_layers([r"sidebar=First\nSecond\n§eThird
literal=Keep \\n and \\ and \t and \q and trailing\
placeholder=Before\n%1\nAfter
"
        .as_bytes()])
        .unwrap();
        assert_eq!(table.lookup("sidebar"), Some("First\nSecond\n§eThird"));
        assert_eq!(
            table.lookup("literal"),
            Some(r"Keep \n and \ and \t and \q and trailing\")
        );
        assert_eq!(table.lookup("placeholder"), Some("Before\n%1\nAfter"));
    }

    #[test]
    fn small_tables_fit_without_reserving_unused_rows() {
        let large =
            ServerLangOverlay::from_layers([format!("large={}\n", "v".repeat(9 * 1024 * 1024))])
                .unwrap();
        let small = ServerLangOverlay::from_layers([b"k=v"]).unwrap();
        assert_eq!(small.lookup("k"), Some("v"));
        assert_eq!(large.lookup("large").unwrap().len(), 9 * 1024 * 1024);
    }

    #[test]
    fn ignored_rows_preserve_valid_definitions() {
        let mut input = "## comment\nmalformed\n=empty\n".repeat(20_000);
        input.push_str("k=v\n");
        let table = ServerLangOverlay::from_layers([input]).unwrap();
        assert_eq!(table.lookup("k"), Some("v"));
    }

    #[test]
    fn invalid_layers_do_not_discard_other_sources() {
        let table = ServerLangOverlay::from_layers([
            b"first=First\nshared=Before".as_slice(),
            b"bad=\xff".as_slice(),
            b"\xef\xbb\xbfshared=After\nlast=Last".as_slice(),
        ])
        .unwrap();
        assert_eq!(table.lookup("first"), Some("First"));
        assert_eq!(table.lookup("shared"), Some("After"));
        assert_eq!(table.lookup("last"), Some("Last"));
    }

    #[test]
    fn snapshots_survive_input_drop_and_later_tables() {
        let input = String::from("k=old");
        let table = ServerLangOverlay::from_layers([input.as_bytes()]).unwrap();
        let retained = Arc::clone(&table);
        drop(input);
        drop(table);
        let new = ServerLangOverlay::from_layers([b"k=new"]).unwrap();
        assert_eq!(retained.lookup("k"), Some("old"));
        assert_eq!(new.lookup("k"), Some("new"));
    }

    #[test]
    fn allocation_read_and_parse_failures_do_not_publish_partial_input() {
        assert!(ServerLangOverlay::read(usize::MAX, |_| panic!("unallocatable input")).is_none());
        assert!(ServerLangOverlay::read(3, |_| false).is_none());
        assert!(ServerLangOverlay::from_layers([b"k=\xff"]).is_none());
        assert!(ServerLangOverlay::from_layers(std::iter::empty::<&[u8]>()).is_none());
    }

    #[test]
    fn retained_candidates_do_not_exhaust_a_process_policy() {
        let input = (0..512)
            .map(|index| format!("key{index}={}\n", "x".repeat(1024)))
            .collect::<String>();
        let retained: Vec<_> = (0..32)
            .map(|_| ServerLangOverlay::from_layers([input.as_bytes()]).unwrap())
            .collect();
        for table in &retained {
            assert_eq!(table.lookup("key0").unwrap().len(), 1024);
        }
        let last = Arc::clone(retained.last().unwrap());
        drop(retained);
        assert_eq!(last.lookup("key511").unwrap().len(), 1024);
    }
}
