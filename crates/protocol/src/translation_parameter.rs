//! Parameter localization shared by TextPacket and raw-text translations.

use std::{borrow::Cow, sync::Arc};

/// Looks up a leading `%` whole key once; unknown keys and other text stay literal.
/// Formatting codes and percent tokens in the returned translation are preserved.
pub fn localize_parameter_prefix<'a>(
    text: &'a str,
    lookup: &dyn Fn(&str) -> Option<Arc<str>>,
    max_bytes: usize,
) -> Cow<'a, str> {
    match text
        .strip_prefix('%')
        .filter(|marked| !marked.is_empty())
        .and_then(|marked| {
            lookup(marked).or_else(|| {
                marked
                    .bytes()
                    .any(|byte| byte.is_ascii_uppercase())
                    .then(|| lookup(&marked.to_ascii_lowercase()))
                    .flatten()
            })
        }) {
        Some(value) => Cow::Owned(prefix(&value, max_bytes).to_owned()),
        None => Cow::Borrowed(prefix(text, max_bytes)),
    }
}

/// Retains complete UTF-8 scalars inside the output budget.
fn prefix(text: &str, max_bytes: usize) -> &str {
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Provides a small language table for parameter lookup tests.
    fn table(key: &str) -> Option<Arc<str>> {
        (key == "menu.play").then(|| Arc::from("Play"))
    }

    #[test]
    fn parameters_only_translate_a_marked_whole_key() {
        for (text, expected) in [
            ("%menu.play", "Play"),
            ("menu.play", "menu.play"),
            ("100% literal %menu.play", "100% literal %menu.play"),
            ("%menu.play!", "%menu.play!"),
            ("%missing.key", "%missing.key"),
            ("%", "%"),
        ] {
            for limit in 0..=expected.len() + 1 {
                assert_eq!(
                    localize_parameter_prefix(text, &table, limit),
                    prefix(expected, limit)
                );
            }
        }
    }

    #[test]
    fn translated_parameter_capacity_respects_the_prefix_budget() {
        let expanded: Arc<str> = "x".repeat(100_000).into();
        let translate = |key: &str| (key == "x").then(|| Arc::clone(&expanded));
        let Cow::Owned(result) = localize_parameter_prefix("%x", &translate, 123) else {
            panic!("translated text is owned")
        };
        assert_eq!(result.len(), 123);
        assert!(result.capacity() <= 123);
    }

    #[test]
    fn parameter_lookup_is_single_pass_and_bounded() {
        let calls = std::cell::Cell::new(0);
        let translate = |key: &str| {
            calls.set(calls.get() + 1);
            (key == "alias").then(|| Arc::from("§a%menu.play🌍"))
        };
        let result = localize_parameter_prefix("%alias", &translate, 15);
        assert_eq!(result, "§a%menu.play");
        assert_eq!(calls.get(), 1);
        let Cow::Owned(result) = result else {
            panic!("translated text is owned")
        };
        assert!(result.capacity() <= 15);
    }
}
