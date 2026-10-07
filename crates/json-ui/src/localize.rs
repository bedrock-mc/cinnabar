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
    let mut token: Option<usize> = None;
    for (at, ch) in text.char_indices() {
        if out.sealed {
            break;
        }
        match token {
            Some(_) if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_') => {}
            Some(start) => {
                substitute(&text[start..at], lookup, &mut out);
                out.push(ch);
                token = None;
            }
            None if ch == '%' => token = Some(at + 1),
            None => out.push(ch),
        }
    }
    if let Some(start) = token {
        substitute(&text[start..], lookup, &mut out);
    }
    Cow::Owned(out.text)
}

/// Expands one label token into the bounded output.
fn substitute(token: &str, lookup: &dyn Fn(&str) -> Option<Arc<str>>, out: &mut TextPrefix) {
    match key(token, lookup) {
        Some(value) => out.push_str(&value),
        None => out.push_str(token),
    }
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

    /// Appends one scalar if the retained prefix has room.
    fn push(&mut self, character: char) {
        let mut bytes = [0; 4];
        self.push_str(character.encode_utf8(&mut bytes));
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
}
