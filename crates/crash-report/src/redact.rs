//! Share-time redaction of logs and crash reports, from the one rule file both languages use.
//!
//! `data/redaction-rules.json` lists credential and private-data rules with conformance
//! vectors; the Go core embeds a byte-for-byte copy (`core/internal/redact/rules.json`) whose
//! `redact.Text` is [`patterns`]. Every removed span becomes [`PLACEHOLDER`].
//!
//! Sharing a whole bundle goes further than one pass: [`Learned`] collects private values from
//! every member (field values, home folder names, JSON under private keys) so a [`Redactor`]
//! removes them wherever else they appear, and both decode JSON and debug escapes so an escaped
//! value cannot slip past a rule.

mod escapes;
mod rules;
#[cfg(test)]
mod tests;

use std::fmt;

use regex::{Regex, RegexBuilder};
use rules::RULES;

/// What every redacted span becomes; equal to the rule file's `placeholder`.
pub const PLACEHOLDER: &str = "[redacted]";

/// The most escape layers redaction decodes; text nested deeper is [`Error::EscapeDepth`]
/// rather than shared.
pub const MAX_ESCAPE_DEPTH: usize = 4;

/// Why text cannot be redacted safely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Escapes nest deeper than redaction decodes, so a value could hide inside.
    EscapeDepth,
    /// Two JSON keys would become the same key once redacted.
    MergedKeys,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EscapeDepth => "text escape depth exceeds the redaction limit",
            Self::MergedKeys => "redaction would merge JSON keys",
        })
    }
}

impl std::error::Error for Error {}

/// One pass of the shared rules over `text`, exactly as the Go core's `redact.Text`: no learned
/// values and no escape decoding.
#[must_use]
pub fn patterns(text: &str) -> String {
    RULES.apply(text, false)
}

/// The shared single-pass rules for a known module filename, retaining filename identity.
pub fn patterns_file_name(text: &str) -> String {
    RULES.apply(text, true)
}

/// Private values learned from text and JSON, to be removed wherever they appear.
#[derive(Debug, Default)]
pub struct Learned {
    values: Vec<String>,
}

impl Learned {
    /// Learns every value a capturing rule would remove from `text` or any decoded layer of it,
    /// and values under private keys of JSON embedded in it.
    pub fn text(&mut self, text: &str) -> Result<(), Error> {
        for view in escapes::views(text)? {
            self.embedded_json(&view);
            for rule in &RULES.rules {
                for captures in rule.pattern.captures_iter(&view) {
                    for group in captures.iter().skip(1).flatten() {
                        self.raw(group.as_str());
                    }
                }
            }
        }
        Ok(())
    }

    /// Learns strings and numbers under private keys anywhere in `value`.
    pub fn json(&mut self, value: &serde_json::Value) {
        self.json_values(value, false);
    }

    /// Learns `value` itself, such as the player's home directory.
    pub fn value(&mut self, value: impl Into<String>) {
        self.push(value.into());
    }

    /// The redactor removing everything learned, in every escaped spelling.
    #[must_use]
    pub fn redactor(self) -> Redactor {
        let mut spellings = Vec::new();
        for value in &self.values {
            escapes::spellings(value, &mut spellings);
        }
        spellings.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        spellings.dedup();
        let values = (!spellings.is_empty()).then(|| {
            let alternation = spellings
                .iter()
                .map(|value| regex::escape(value))
                .collect::<Vec<_>>()
                .join("|");
            RegexBuilder::new(&alternation)
                .case_insensitive(true)
                .size_limit(1 << 28)
                .build()
                .expect("escaped literals compile")
        });
        Redactor { values }
    }

    /// Learns a captured value: JSON-decoded when quoted, plus its credential after a scheme.
    fn raw(&mut self, raw: &str) {
        // A trailing backslash is a cut escape; a decoded view learns the whole value.
        if raw.ends_with('\\') {
            return;
        }
        let value = serde_json::from_str::<String>(raw)
            .unwrap_or_else(|_| raw.trim_matches(['"', '\'']).to_owned());
        if let Some((_, credential)) = value.trim().split_once([' ', '\t']) {
            self.push(credential.trim().to_owned());
        }
        self.push(value);
    }

    fn push(&mut self, value: String) {
        if !value.is_empty() && !value.starts_with(&PLACEHOLDER[..PLACEHOLDER.len() - 1]) {
            self.values.push(value);
        }
    }

    /// Learns private values from JSON objects and arrays embedded in text.
    fn embedded_json(&mut self, text: &str) {
        let mut start = 0;
        while let Some(offset) = text[start..].find(['{', '[']) {
            let at = start + offset;
            let mut stream =
                serde_json::Deserializer::from_str(&text[at..]).into_iter::<serde_json::Value>();
            start = match stream.next() {
                Some(Ok(value)) => {
                    self.json_values(&value, false);
                    at + stream.byte_offset()
                }
                _ => at + 1,
            };
        }
    }

    fn json_values(&mut self, value: &serde_json::Value, private: bool) {
        match value {
            serde_json::Value::String(text) if private => self.push(text.clone()),
            serde_json::Value::Number(number) if private => self.push(number.to_string()),
            serde_json::Value::Array(items) => {
                for item in items {
                    self.json_values(item, private);
                }
            }
            serde_json::Value::Object(items) => {
                for (key, item) in items {
                    self.json_values(item, private || RULES.field_key.is_match(key));
                }
            }
            _ => {}
        }
    }
}

/// Removes learned values and applies the shared rules to text and every decoded layer of it.
/// The default redactor has learned nothing.
#[derive(Debug, Default)]
pub struct Redactor {
    values: Option<Regex>,
}

impl Redactor {
    /// `text` scrubbed at every escape layer. The result is the shallowest layer that needed no
    /// further redaction, so literal backslashes such as Windows paths survive unless a deeper
    /// decode revealed something private.
    pub fn text(&self, text: &str) -> Result<String, Error> {
        self.transform(text, false)
    }

    /// Applies escape decoding and learned values in either ordinary text or filename context.
    fn transform(&self, text: &str, file_name: bool) -> Result<String, Error> {
        let mut layer = self.scrub(&escapes::strip_ansi(text), file_name);
        let mut emitted = layer.clone();
        for _ in 0..MAX_ESCAPE_DEPTH {
            let decoded = escapes::strip_ansi(&escapes::decode(&layer));
            if decoded == layer {
                return Ok(emitted);
            }
            // A later decode can disguise a value, such as turning `\b` into a backspace.
            layer = self.scrub(&decoded, file_name);
            if layer != decoded {
                emitted.clone_from(&layer);
            }
        }
        if escapes::strip_ansi(&escapes::decode(&layer)) != layer {
            return Err(Error::EscapeDepth);
        }
        Ok(emitted)
    }

    /// Redacts a known module filename without treating the filename itself as a hostname.
    pub fn file_name(&self, text: &str) -> Result<String, Error> {
        self.transform(text, true)
    }

    /// Scrubs every string in `value` and replaces values under private keys whole, keeping
    /// the document's structure.
    pub fn json(&self, value: &mut serde_json::Value) -> Result<(), Error> {
        match value {
            serde_json::Value::String(text) => *text = self.text(text)?,
            serde_json::Value::Array(values) => {
                for value in values {
                    self.json(value)?;
                }
            }
            serde_json::Value::Object(values) => {
                for (key, mut value) in std::mem::take(values) {
                    let decoded = escapes::views(&key)?;
                    if RULES
                        .field_key
                        .is_match(decoded.last().expect("views start non-empty"))
                    {
                        value = serde_json::Value::String(PLACEHOLDER.into());
                    } else {
                        self.json(&mut value)?;
                    }
                    if values.insert(self.text(&key)?, value).is_some() {
                        return Err(Error::MergedKeys);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Learned values first, before decoding can alter quote or backslash boundaries, then the
    /// shared rules.
    fn scrub(&self, text: &str, file_name: bool) -> String {
        match &self.values {
            Some(values) => RULES.apply(&values.replace_all(text, PLACEHOLDER), file_name),
            None => RULES.apply(text, file_name),
        }
    }
}
