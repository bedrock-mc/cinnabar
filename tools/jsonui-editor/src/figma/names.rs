//! The layer-name convention: words name the control, and `@`/`$` tokens add
//! Bedrock concepts Figma has no layer for.

use std::collections::BTreeSet;

/// The directive asking the plugin to export a layer as one texture.
pub(super) const IMAGE: &str = "@image";
/// The directive making a layer a button; the plugin sends a button
/// instance's variants with it.
pub(super) const BUTTON: &str = "@button";

/// A layer name split into its words and directives.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Directives {
    /// The name without directives.
    pub words: String,
    /// `@button` or `@button=<id>`: `Some(None)` uses the control name as the id.
    pub button: Option<Option<String>>,
    /// `@image`: the plugin exported the layer as one texture.
    pub image: bool,
    /// `@factory=<collection>`.
    pub factory: Option<String>,
    /// `$binding=#name`: binds a label's text or an image's texture.
    pub binding: Option<String>,
    /// `$visible=#name`.
    pub visible: Option<String>,
    /// Tokens that look like directives but are not ones this convention defines.
    pub unknown: Vec<String>,
}

pub(super) fn parse(name: &str) -> Directives {
    let mut out = Directives::default();
    let mut words = Vec::new();
    for token in name.split_whitespace() {
        if !token.starts_with(['@', '$']) {
            words.push(token);
            continue;
        }
        let (key, value) = match token.split_once('=') {
            Some((key, value)) => (key, Some(value)),
            None => (token, None),
        };
        let binding = || value.and_then(binding_name);
        match (key, value) {
            (BUTTON, None) => out.button = Some(None),
            (BUTTON, Some(id)) if !identifier(id, "").is_empty() => {
                out.button = Some(Some(identifier(id, "")));
            }
            (IMAGE, None) => out.image = true,
            ("@factory", Some(collection)) if !identifier(collection, "").is_empty() => {
                out.factory = Some(identifier(collection, ""));
            }
            ("$binding", Some(_)) if binding().is_some() => out.binding = binding(),
            ("$visible", Some(_)) if binding().is_some() => out.visible = binding(),
            _ => out.unknown.push(token.to_owned()),
        }
    }
    out.words = words.join(" ");
    out
}

/// `#name` from `#name` or `name`, exactly as written: JSON-UI names are case-sensitive.
fn binding_name(value: &str) -> Option<String> {
    let name = value.trim_start_matches('#');
    (!name.is_empty()).then(|| format!("#{name}"))
}

/// `text` as a lowercase JSON-UI identifier, else `fallback`.
pub(super) fn identifier(text: &str, fallback: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_');
    match out.chars().next() {
        None => fallback.to_owned(),
        Some(first) if first.is_ascii_digit() && !fallback.is_empty() => {
            format!("{fallback}_{out}")
        }
        Some(_) => out.to_owned(),
    }
}

/// Names already used in one scope.
#[derive(Default)]
pub(super) struct NameSet(BTreeSet<String>);

impl NameSet {
    /// `base`, or `base_2`, `base_3`, ... when taken.
    pub fn unique(&mut self, base: &str) -> String {
        let name = (1..)
            .map(|n| match n {
                1 => base.to_owned(),
                n => format!("{base}_{n}"),
            })
            .find(|name| !self.0.contains(name))
            .expect("an unbounded range has a free name");
        self.0.insert(name.clone());
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directives_split_from_words() {
        let parsed = parse("Play Game @button=play_game $binding=#title $visible=shown @shiny");
        assert_eq!(parsed.words, "Play Game");
        assert_eq!(parsed.button, Some(Some("play_game".into())));
        assert_eq!(parsed.binding.as_deref(), Some("#title"));
        assert_eq!(parsed.visible.as_deref(), Some("#shown"));
        assert_eq!(parsed.unknown, ["@shiny"]);
    }

    #[test]
    fn identifiers_are_snake_case_and_never_start_with_a_digit() {
        assert_eq!(identifier("Main Menu!", "x"), "main_menu");
        assert_eq!(identifier("3 Buttons", "layer"), "layer_3_buttons");
        assert_eq!(identifier("  ", "frame"), "frame");
        let mut names = NameSet::default();
        assert_eq!(
            ["a", "a", "a"].map(|base| names.unique(base)),
            ["a", "a_2", "a_3"]
        );
    }
}
