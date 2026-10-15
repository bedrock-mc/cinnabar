//! Escape layers: log text can carry JSON or debug-quoted strings whose escapes hide values.

use std::sync::LazyLock;

use regex::Regex;

use super::{Error, MAX_ESCAPE_DEPTH};

/// Removes ANSI styling, which a terminal formatter can place between a field name and `=`.
pub(super) fn strip_ansi(text: &str) -> String {
    static ANSI: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("valid pattern"));
    ANSI.replace_all(text, "").into_owned()
}

/// Decodes JSON and common debug escapes without requiring the surrounding text to be JSON.
pub(super) fn decode(text: &str) -> String {
    static ESCAPES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"\\(?:u\{[0-9a-fA-F]{1,6}\}|x[0-9a-fA-F]{2}|u[0-9a-fA-F]{4}(?:\\u[0-9a-fA-F]{4})?|["\\/bfnrt])"#)
            .expect("valid pattern")
    });
    ESCAPES
        .replace_all(text, |capture: &regex::Captures<'_>| {
            let escaped = &capture[0];
            let hex = escaped.strip_prefix("\\x").or_else(|| {
                escaped
                    .strip_prefix("\\u{")
                    .and_then(|value| value.strip_suffix('}'))
            });
            if let Some(value) = hex
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .and_then(char::from_u32)
            {
                value.to_string()
            } else {
                serde_json::from_str::<String>(&format!("\"{escaped}\""))
                    .unwrap_or_else(|_| escaped.into())
            }
        })
        .into_owned()
}

/// The ANSI-free text and each decoded layer, shallowest first; text nested deeper than
/// [`MAX_ESCAPE_DEPTH`] is an error.
pub(super) fn views(text: &str) -> Result<Vec<String>, Error> {
    let mut views = vec![strip_ansi(text)];
    for _ in 0..MAX_ESCAPE_DEPTH {
        let last = views.last().expect("views start non-empty");
        let next = strip_ansi(&decode(last));
        if next == *last {
            return Ok(views);
        }
        views.push(next);
    }
    let last = views.last().expect("views start non-empty");
    if decode(last) != *last {
        return Err(Error::EscapeDepth);
    }
    Ok(views)
}

/// Every literal spelling of `value` up to the escape depth, including quote-only escaping.
pub(super) fn spellings(value: &str, out: &mut Vec<String>) {
    out.push(value.replace('"', "\\\""));
    let mut escaped = value.to_owned();
    for _ in 0..=MAX_ESCAPE_DEPTH {
        let json = serde_json::to_string(&escaped).expect("strings serialize");
        out.push(std::mem::replace(
            &mut escaped,
            json[1..json.len() - 1].into(),
        ));
    }
}
