//! Label localization as the vanilla client applies it: text
//! without `%` is one whole key; otherwise each `%token` (ASCII letters,
//! digits, `-`, `.`, `_`) is replaced by its translation or, when missing, by
//! its own text, so an empty token drops its `%`. The character ending a token
//! is kept as is. Keys match exactly, then lowercased.

use std::{borrow::Cow, sync::Arc};

/// `text` localized through `lookup` (the active language table).
pub fn localize_text<'a>(text: &'a str, lookup: &dyn Fn(&str) -> Option<Arc<str>>) -> Cow<'a, str> {
    localize_text_prefix(text, lookup, usize::MAX)
}

/// Localizes only the retained UTF-8 prefix, stopping at the first omitted scalar.
pub fn localize_text_prefix<'a>(
    text: &'a str,
    lookup: &dyn Fn(&str) -> Option<Arc<str>>,
    max_bytes: usize,
) -> Cow<'a, str> {
    if text.is_empty() {
        return Cow::Borrowed(text);
    }
    if !text.contains('%') {
        return match key(text, lookup) {
            Some(value) => Cow::Owned(prefix(&value, max_bytes).to_owned()),
            None => Cow::Borrowed(prefix(text, max_bytes)),
        };
    }
    let mut out = TextPrefix::new(text.len(), max_bytes);
    let mut chunks = localized_chunks(text, lookup);
    while !out.sealed {
        let Some(chunk) = chunks.next() else {
            break;
        };
        out.push_str(chunk.as_str());
    }
    Cow::Owned(out.text)
}

/// Yields localized scalars on demand, retaining at most one lookup result at a time.
/// Returned translations are literal chunks and are never localized recursively.
pub fn localize_text_chars<'a>(
    text: &'a str,
    lookup: &'a dyn Fn(&str) -> Option<Arc<str>>,
) -> impl Iterator<Item = char> + 'a {
    let mut chunks = localized_chunks(text, lookup);
    let mut current = LocalizedChunk::Literal("");
    let mut offset = 0;
    std::iter::from_fn(move || {
        loop {
            if let Some(character) = current.as_str()[offset..].chars().next() {
                offset += character.len_utf8();
                return Some(character);
            }
            // Release the consumed translation before looking up the next one.
            drop(std::mem::replace(&mut current, LocalizedChunk::Literal("")));
            offset = 0;
            current = chunks.next()?;
        }
    })
}

enum LocalizedChunk<'a> {
    Literal(&'a str),
    Translation(Arc<str>),
}

impl LocalizedChunk<'_> {
    /// Borrows either input text or the current shared translation.
    fn as_str(&self) -> &str {
        match self {
            Self::Literal(text) => text,
            Self::Translation(text) => text,
        }
    }
}

enum LocalizationPart {
    WholeKey,
    Literal,
    Token,
    Delimiter,
}

/// Shares whole-key lookup and percent-token parsing between collecting and streaming callers.
fn localized_chunks<'a>(
    mut text: &'a str,
    lookup: &'a dyn Fn(&str) -> Option<Arc<str>>,
) -> impl Iterator<Item = LocalizedChunk<'a>> + 'a {
    let mut part = if text.contains('%') {
        LocalizationPart::Literal
    } else {
        LocalizationPart::WholeKey
    };
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let (end, translated) = match part {
            LocalizationPart::WholeKey => (text.len(), true),
            LocalizationPart::Literal => {
                let end = text.find('%').unwrap_or(text.len());
                let literal = &text[..end];
                text = text.get(end + 1..).unwrap_or("");
                part = LocalizationPart::Token;
                return Some(LocalizedChunk::Literal(literal));
            }
            LocalizationPart::Token => {
                part = LocalizationPart::Delimiter;
                let end = text
                    .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '-' | '.' | '_'))
                    .unwrap_or(text.len());
                (end, true)
            }
            LocalizationPart::Delimiter => {
                part = LocalizationPart::Literal;
                (text.chars().next()?.len_utf8(), false)
            }
        };
        let chunk = &text[..end];
        text = &text[end..];
        Some(if translated {
            key(chunk, lookup).map_or(LocalizedChunk::Literal(chunk), LocalizedChunk::Translation)
        } else {
            LocalizedChunk::Literal(chunk)
        })
    })
}

/// Keeps complete scalars inside the byte budget.
fn prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

struct TextPrefix {
    text: String,
    max_bytes: usize,
    sealed: bool,
}

impl TextPrefix {
    /// Reserves only capacity the retained prefix can use.
    fn new(input_bytes: usize, max_bytes: usize) -> Self {
        Self {
            text: String::with_capacity(input_bytes.min(max_bytes)),
            max_bytes,
            sealed: false,
        }
    }

    /// Stops permanently at the first scalar outside the budget.
    fn push_str(&mut self, text: &str) {
        if self.sealed {
            return;
        }
        let retained = prefix(text, self.max_bytes - self.text.len());
        let required = self.text.len() + retained.len();
        if required > self.text.capacity() {
            let geometric = self.text.capacity().saturating_mul(2).max(16);
            let target = required.max(geometric).min(self.max_bytes);
            self.text.reserve_exact(target - self.text.len());
        }
        self.text.push_str(retained);
        self.sealed = retained.len() < text.len();
    }
}

fn key(text: &str, lookup: &dyn Fn(&str) -> Option<Arc<str>>) -> Option<Arc<str>> {
    if text.is_empty() {
        return None;
    }
    lookup(text).or_else(|| {
        text.bytes()
            .any(|byte| byte.is_ascii_uppercase())
            .then(|| lookup(&text.to_ascii_lowercase()))
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(key: &str) -> Option<Arc<str>> {
        match key {
            "menu.play" => Some("Play".into()),
            "trial.pausescreen.buygame" => Some("Unlock Full Game".into()),
            _ => None,
        }
    }

    #[test]
    fn whole_keys_tokens_and_unknown_text_follow_the_vanilla_rules() {
        assert_eq!(localize_text("menu.play", &table), "Play");
        assert_eq!(
            localize_text("trial.pauseScreen.buyGame", &table),
            "Unlock Full Game"
        );
        assert_eq!(localize_text("Hello there", &table), "Hello there");
        assert_eq!(localize_text("§l%menu.play!", &table), "§lPlay!");
        assert_eq!(localize_text("%missing.key x", &table), "missing.key x");
        assert_eq!(localize_text("100% sure", &table), "100 sure");
        assert_eq!(localize_text("%", &table), "");
        assert_eq!(
            localize_text("%menu.play%menu.play", &table),
            "Play%menu.play"
        );
        let long = format!("k{}", "a".repeat(256));
        let found = |key: &str| (key == long).then(|| Arc::from("long"));
        assert_eq!(localize_text(&long, &found), "long");
    }

    #[test]
    fn localized_prefix_preserves_token_rules_and_whole_key_lookup() {
        for (text, expected) in [
            ("menu.play", "Play"),
            ("trial.pauseScreen.buyGame", "Unlock Full Game"),
            ("Hello there", "Hello there"),
            ("§l%menu.play!", "§lPlay!"),
            ("%missing.key x", "missing.key x"),
            ("100% sure", "100 sure"),
            ("%", ""),
            ("%menu.play%menu.play", "Play%menu.play"),
        ] {
            for max_bytes in 0..=expected.len() + 1 {
                let mut end = max_bytes.min(expected.len());
                while !expected.is_char_boundary(end) {
                    end -= 1;
                }
                assert_eq!(
                    localize_text_prefix(text, &table, max_bytes),
                    &expected[..end],
                    "text={text:?}, max_bytes={max_bytes}"
                );
            }
        }
    }

    #[test]
    fn localized_prefix_seals_at_the_first_omitted_scalar() {
        let max_bytes = 7;
        for scalar in ["é", "世", "🌍"] {
            for remaining in 0..scalar.len() {
                let initial = "a".repeat(max_bytes - remaining);
                let translated: Arc<str> = format!("{initial}{scalar}Z").into();
                let lookup = |key: &str| (key == "x").then(|| Arc::clone(&translated));
                let localized = localize_text_prefix("%x!suffix", &lookup, max_bytes);
                assert_eq!(localized, initial);
                let Cow::Owned(localized) = localized else {
                    panic!("a translated token owns its retained output");
                };
                assert!(localized.capacity() <= max_bytes);
            }
        }
    }

    #[test]
    fn localized_scalars_preserve_whole_keys_tokens_and_delimiters() {
        for text in [
            "",
            "menu.play",
            "trial.pauseScreen.buyGame",
            "Hello 🌍",
            "§l%menu.play!",
            "%missing.key x",
            "100% sure",
            "%",
            "%%",
            "%menu.play%menu.play",
            "%menu.play!%menu.play",
            "%!%menu.play",
            "%menu.play🌍%menu.play",
        ] {
            let streamed: String = localize_text_chars(text, &table).collect();
            assert_eq!(streamed, localize_text(text, &table), "{text:?}");
        }
    }

    #[test]
    fn localized_scalars_lookup_lazily_and_release_each_consumed_value() {
        let value: Arc<str> = Arc::from("é🌍%nested");
        let calls = std::cell::Cell::new(0);
        let lookup = |_: &str| {
            assert_eq!(Arc::strong_count(&value), 1);
            calls.set(calls.get() + 1);
            Some(Arc::clone(&value))
        };
        let mut characters = localize_text_chars("%x %x %x", &lookup);
        assert_eq!(calls.get(), 0);
        let (prefix, allocations) = test_alloc::measure(|| characters.by_ref().take(2).count());
        assert_eq!(prefix, 2);
        assert_eq!(allocations.calls, 0);
        assert_eq!(calls.get(), 1);
        assert_eq!(Arc::strong_count(&value), 2);
        drop(characters);
        assert_eq!(Arc::strong_count(&value), 1);
        let mut characters = localize_text_chars("%x %x %x", &lookup);
        assert_eq!(
            characters.by_ref().collect::<String>(),
            format!("{value} {value} {value}")
        );
        assert_eq!(calls.get(), 4);
        assert_eq!(Arc::strong_count(&value), 1);
        assert_eq!(characters.next(), None);
    }
}
