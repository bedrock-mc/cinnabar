//! Preprocesses properties as one token stream, then reads Java property entries.

use std::collections::{BTreeMap, BTreeSet};

use super::PropertiesDiagnostic;
use crate::source::{
    MAX_EXPANDED_PROPERTIES_BYTES, MAX_PROPERTIES_PREPROCESSING_BYTES,
    MAX_PROPERTIES_PREPROCESSING_TOKENS,
};
use pp_rs::pp::Preprocessor;

/// Stands in for a line whose preprocessing failed, so its logical entry is discarded.
const REJECTED: char = '\0';

/// One key/value pair from an active logical line.
#[derive(Clone)]
pub(crate) struct RawEntry {
    /// 1-based line where the logical line starts.
    pub(crate) line: u32,
    pub(crate) key: String,
    pub(crate) value: String,
}

/// Runs one preprocessor over the complete file; layout metadata uses the original values.
pub(crate) fn read_entries(
    text: &str,
    defines: &BTreeMap<String, String>,
    diagnostics: &mut Vec<PropertiesDiagnostic>,
) -> Vec<RawEntry> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let source: String = text
        .lines()
        .map(|line| {
            let line = line.trim();
            let kept = if let Some(rest) = line.strip_prefix('#') {
                let rest = rest.trim_start();
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                if Preprocessor::is_directive(&rest[..end]) && !rest[end..].starts_with(['_', '.'])
                {
                    line.to_owned()
                } else {
                    String::new()
                }
            } else {
                line.replace('#', "")
            };
            kept + "\n"
        })
        .collect();
    let mut pp = Preprocessor::new(&source);
    pp.set_work_limit(MAX_PROPERTIES_PREPROCESSING_TOKENS);
    pp.set_iris_conditions(true);
    pp.set_preserve_text(true);
    for (name, value) in defines {
        if let Err((error, _)) = pp.add_define(name, if value.is_empty() { "1" } else { value }) {
            diagnostics.push(PropertiesDiagnostic {
                line: 1,
                message: format!("invalid macro {name}: {error:?}"),
            });
        }
    }
    let mut lines = BTreeMap::<u32, String>::new();
    let mut rejected = BTreeSet::new();
    let mut remaining = MAX_EXPANDED_PROPERTIES_BYTES;
    let mut work = MAX_PROPERTIES_PREPROCESSING_BYTES;
    while let Some(token) = pp.next_text() {
        match token {
            Ok(token) => {
                // Rejected lines still expand, so recursive macros need a budget of their own.
                let Some(left) = work.checked_sub(token.text.len().max(1)) else {
                    diagnostics.push(PropertiesDiagnostic {
                        line: token.line,
                        message: "properties preprocessing exceeds work limit".to_owned(),
                    });
                    break;
                };
                work = left;
                for (index, fragment) in token.text.split('\n').enumerate() {
                    let line = token.line.saturating_add(index as u32);
                    if fragment.is_empty() || rejected.contains(&line) {
                        continue;
                    }
                    if fragment.len() > remaining {
                        remaining += lines.remove(&line).map_or(0, |text| text.len());
                        rejected.insert(line);
                        diagnostics.push(PropertiesDiagnostic {
                            line,
                            message: "properties macro expansion exceeds byte limit".to_owned(),
                        });
                    } else {
                        remaining -= fragment.len();
                        lines.entry(line).or_default().push_str(fragment);
                    }
                }
            }
            Err((error, location)) => {
                // A failed expansion invalidates its whole entry, not just the remaining text.
                lines.remove(&location.line());
                rejected.insert(location.line());
                if pp.work_remaining() == 0 {
                    diagnostics.push(PropertiesDiagnostic {
                        line: location.line(),
                        message: "properties preprocessing exceeds work limit".to_owned(),
                    });
                    break;
                }
                diagnostics.push(PropertiesDiagnostic {
                    line: location.line(),
                    message: format!("invalid properties preprocessing: {error:?}"),
                });
            }
        }
    }
    let mut entries: Vec<_> =
        logical_entries(text.lines().enumerate().filter_map(|(index, physical)| {
            if physical.trim().is_empty() {
                return None;
            }
            let line = (index + 1) as u32;
            if rejected.contains(&line) {
                // Keep the continuation so the marker poisons every line of the entry.
                let continued = physical.trim_end().ends_with('\\');
                return Some((
                    line,
                    format!("{REJECTED}{}", if continued { "\\" } else { "" }),
                ));
            }
            Some((line, lines.remove(&line).unwrap_or_default()))
        }))
        .filter(|entry| !raw_key(&entry.key))
        .filter(|entry| !entry.key.contains(REJECTED) && !entry.value.contains(REJECTED))
        .collect();
    entries.extend(
        logical_entries(text.lines().enumerate().filter_map(|(index, line)| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            Some((
                (index + 1) as u32,
                if line.starts_with('#') {
                    String::new()
                } else {
                    line.to_owned()
                },
            ))
        }))
        .filter(|entry| raw_key(&entry.key)),
    );
    entries.sort_by_key(|entry| entry.line);
    entries
}

/// Layout and profile metadata names options instead of their expanded values.
fn raw_key(key: &str) -> bool {
    matches!(key, "screen" | "sliders") || key.starts_with("screen.") || key.starts_with("profile.")
}

/// Reads property continuations after preprocessing, with source line numbers retained.
fn logical_entries(
    lines: impl IntoIterator<Item = (u32, String)>,
) -> impl Iterator<Item = RawEntry> {
    let mut lines = lines.into_iter();
    let mut pending: Option<(u32, String)> = None;
    std::iter::from_fn(move || {
        loop {
            let Some((line, text)) = lines.next() else {
                return pending
                    .take()
                    .map(|(start, logical)| parse_entry(start, &logical));
            };
            let text = text.trim();
            if let Some((start, mut logical)) = pending.take() {
                match strip_continuation(text) {
                    Some(head) => {
                        logical.push_str(head);
                        pending = Some((start, logical));
                    }
                    None => {
                        logical.push_str(text);
                        return Some(parse_entry(start, &logical));
                    }
                }
                continue;
            }
            if text.is_empty() || text.starts_with(['!', '#']) {
                continue;
            }
            match strip_continuation(text) {
                Some(head) => pending = Some((line, head.to_owned())),
                None => return Some(parse_entry(line, text)),
            }
        }
    })
}

/// `text` without its continuation backslash when it ends in an odd number of `\`.
fn strip_continuation(text: &str) -> Option<&str> {
    let backslashes = text.bytes().rev().take_while(|&b| b == b'\\').count();
    if backslashes.is_multiple_of(2) {
        None
    } else {
        Some(&text[..text.len() - 1])
    }
}

/// Splits a logical line into key and value: the key ends at the first unescaped `=`,
/// `:` or whitespace; whitespace and one `=`/`:` separator follow; both sides are
/// unescaped and trimmed.
fn parse_entry(line: u32, logical: &str) -> RawEntry {
    let mut key_end = logical.len();
    let mut escaped = false;
    for (index, c) in logical.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '=' || c == ':' || c.is_whitespace() {
            key_end = index;
            break;
        }
    }
    let mut value = logical[key_end..].trim_start();
    if let Some(stripped) = value.strip_prefix(['=', ':']) {
        value = stripped.trim_start();
    }
    RawEntry {
        line,
        key: unescape(&logical[..key_end]).trim().to_owned(),
        value: unescape(value).trim().to_owned(),
    }
}

/// Java properties escapes: `\t \n \r \f`, `\uXXXX`, and `\c` → `c` for anything else.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let rest = chars.as_str();
                if let Some(hex) = rest.get(..4)
                    && let Ok(code) = u32::from_str_radix(hex, 16)
                    && let Some(decoded) = char::from_u32(code)
                {
                    out.push(decoded);
                    chars = rest[4..].chars();
                } else {
                    out.push('u');
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}
