//! How the client chooses a manifest's parser from its `format_version`.
//!
//! The client reads `format_version` twice. It first reads the raw value as a semantic version: a
//! `[major, minor, patch]` array of unsigned integers, or a string or number whose literal text follows
//! the semantic version grammar. Versions below 3.0.0 go to the legacy parser and 3.0.0 to the current
//! one; anything else is unsupported. The legacy parser then reads the same value as a JSON integer:
//! numbers truncate toward zero, while strings and arrays read as 0, which selects the deprecated format 0
//! layout whose header names the pack `pack_id` and `packs_version` and carries its modules.

use serde_json::Value;

/// A semantic version as the client parses it; build metadata is not part of its identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SemVersion {
    pub(crate) triple: [u32; 3],
    pub(crate) pre_release: Option<String>,
}

impl SemVersion {
    /// Parses `major[.minor[.patch[-pre][+build]]]`: each number fits an unsigned 16-bit component without a
    /// leading zero, missing parts are zero, and the whole text must match.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let (core, build) = match text.split_once('+') {
            Some((core, build)) => (core, Some(build)),
            None => (text, None),
        };
        let (numbers, pre_release) = match core.split_once('-') {
            Some((numbers, pre)) => (numbers, Some(pre)),
            None => (core, None),
        };
        let parts: Vec<&str> = numbers.split('.').collect();
        if parts.len() > 3 || ((pre_release.is_some() || build.is_some()) && parts.len() != 3) {
            return None;
        }
        let mut triple = [0; 3];
        for (slot, part) in triple.iter_mut().zip(&parts) {
            *slot = number(part)?;
        }
        if pre_release.is_some_and(|pre| !pre.split('.').all(pre_release_identifier))
            || build.is_some_and(|build| !build.split('.').all(identifier))
        {
            return None;
        }
        Some(Self {
            triple,
            pre_release: pre_release.map(str::to_owned),
        })
    }

    /// Whether this version sorts below `triple` with no pre-release, as semantic versions order.
    fn below(&self, triple: [u32; 3]) -> bool {
        self.triple < triple || (self.triple == triple && self.pre_release.is_some())
    }
}

/// `0` or one to five digits without a leading zero.
fn number(part: &str) -> Option<u32> {
    let valid = part == "0"
        || (!part.is_empty()
            && part.len() <= 5
            && !part.starts_with('0')
            && part.bytes().all(|byte| byte.is_ascii_digit()));
    valid
        .then(|| part.parse::<u16>().ok().map(u32::from))
        .flatten()
}

/// A non-empty run of ASCII alphanumerics and hyphens.
fn identifier(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// A pre-release identifier: numeric identifiers carry no leading zero.
fn pre_release_identifier(part: &str) -> bool {
    identifier(part)
        && (part == "0"
            || !part.starts_with('0')
            || part.bytes().any(|byte| !byte.is_ascii_digit()))
}

/// The manifest layout the client parses a pack with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Layout {
    /// Deprecated format 0: identity and modules live in the header.
    Legacy0,
    /// Formats 1 and 2.
    Legacy,
    /// Format 3.
    Current,
}

impl Layout {
    /// The header keys naming the pack's UUID and version.
    pub(crate) fn identity_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::Legacy0 => ("pack_id", "packs_version"),
            Self::Legacy | Self::Current => ("uuid", "version"),
        }
    }

    /// The modules array the layout reads.
    pub(crate) fn modules(self, root: &Value) -> &Value {
        match self {
            Self::Legacy0 => &root["header"]["modules"],
            Self::Legacy | Self::Current => &root["modules"],
        }
    }
}

/// Selects the layout from the manifest's source text and parsed value; `None` is unsupported.
pub(crate) fn layout(source: &[u8], root: &Value) -> Option<Layout> {
    let version = match raw_format_version(source) {
        Some(raw) => semantic_format_version(raw)?,
        // Without a format version the legacy parser reads format 0.
        None => return Some(Layout::Legacy0),
    };
    if version.below([0, 0, 0]) {
        return None;
    }
    if version.below([3, 0, 0]) {
        let format = match &root["format_version"] {
            Value::Number(number) => number
                .as_i64()
                .or_else(|| number.as_f64().map(|real| real.trunc() as i64))?,
            _ => 0,
        };
        return match format {
            0 => Some(Layout::Legacy0),
            1 | 2 => Some(Layout::Legacy),
            _ => None,
        };
    }
    (version.triple == [3, 0, 0]).then_some(Layout::Current)
}

/// The value text after the first `"format_version"` key in the source, as the client finds it: an
/// array runs to its closing bracket, anything else to the next `,` or `}`.
fn raw_format_version(source: &[u8]) -> Option<&[u8]> {
    const KEY: &[u8] = b"\"format_version\"";
    let start = source.windows(KEY.len()).position(|window| window == KEY)? + KEY.len();
    let rest = &source[start..];
    let rest = &rest[rest.iter().position(|&byte| byte == b':')? + 1..];
    let rest = &rest[rest
        .iter()
        .position(|&byte| byte != b' ' && byte != b'\t')?..];
    let end = if rest.first() == Some(&b'[') {
        rest.iter().position(|&byte| byte == b']')? + 1
    } else {
        rest.iter()
            .position(|&byte| byte == b',' || byte == b'}')
            .unwrap_or(rest.len())
    };
    Some(&rest[..end])
}

/// Reads the raw value as the client's semantic format version: arrays of three unsigned integers,
/// strings, and numbers by their literal text.
fn semantic_format_version(raw: &[u8]) -> Option<SemVersion> {
    let text = std::str::from_utf8(raw).ok()?.trim();
    match serde_json::from_str::<Value>(text).ok()? {
        Value::Array(parts) if parts.len() == 3 => {
            let mut triple = [0; 3];
            for (slot, part) in triple.iter_mut().zip(&parts) {
                // Format arrays narrow each unsigned component before selecting a parser.
                *slot = u32::from(u32::try_from(part.as_u64()?).ok()? as u16);
            }
            Some(SemVersion {
                triple,
                pre_release: None,
            })
        }
        Value::String(text) => SemVersion::parse(&text),
        Value::Number(_) => SemVersion::parse(text),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_of(format: &str) -> Option<Layout> {
        let text = format!(r#"{{"format_version": {format}, "header": {{}}}}"#);
        layout(
            text.as_bytes(),
            &serde_json::from_str(&text).unwrap_or(Value::Null),
        )
    }

    #[test]
    fn numbers_select_their_truncated_legacy_format() {
        for (format, expected) in [
            ("1", Layout::Legacy),
            ("2", Layout::Legacy),
            ("2.0", Layout::Legacy),
            ("2.5", Layout::Legacy),
            ("1.9", Layout::Legacy),
            ("0", Layout::Legacy0),
            ("0.5", Layout::Legacy0),
            ("3", Layout::Current),
            ("3.0", Layout::Current),
        ] {
            assert_eq!(layout_of(format), Some(expected), "format {format}");
        }
        for format in ["2e0", "-2", "02", "3.1", "4", "1e100", "true", "null", "{}"] {
            assert_eq!(layout_of(format), None, "format {format}");
        }
    }

    #[test]
    fn strings_and_arrays_below_three_read_as_format_zero() {
        for format in [
            r#""2""#,
            r#""2.0""#,
            r#""2.0.0""#,
            "[2, 0, 0]",
            r#""3.0.0-beta""#,
        ] {
            assert_eq!(layout_of(format), Some(Layout::Legacy0), "format {format}");
        }
        for format in [r#""3""#, r#""3.0.0""#, "[3, 0, 0]"] {
            assert_eq!(layout_of(format), Some(Layout::Current), "format {format}");
        }
        for format in [
            r#""x""#,
            r#""4.0""#,
            "[2, 0]",
            "[2.0, 0, 0]",
            r#""2.0-beta""#,
        ] {
            assert_eq!(layout_of(format), None, "format {format}");
        }
    }

    #[test]
    fn a_missing_format_version_is_format_zero() {
        let text = r#"{"header": {}}"#;
        assert_eq!(
            layout(text.as_bytes(), &serde_json::from_str(text).unwrap()),
            Some(Layout::Legacy0)
        );
    }

    #[test]
    fn semantic_version_components_stop_at_u16_max() {
        for text in ["65536", "1.65536", "1.2.65536", "99999.0.0"] {
            assert_eq!(SemVersion::parse(text), None, "{text}");
        }
        assert_eq!(
            SemVersion::parse("65535.65535.65535").unwrap().triple,
            [65535; 3]
        );
    }

    #[test]
    fn prereleases_below_the_first_manifest_slice_are_unsupported() {
        assert_eq!(layout_of(r#""0.0.0-beta""#), None);
        assert_eq!(layout_of(r#""0.0.0""#), Some(Layout::Legacy0));
        assert_eq!(layout_of(r#""0.0.1-beta""#), Some(Layout::Legacy0));
    }

    #[test]
    fn format_arrays_narrow_components_before_selecting_the_layout() {
        for (format, expected) in [
            ("[65536, 0, 0]", Layout::Legacy0),
            ("[65539, 0, 0]", Layout::Current),
            ("[3, 65536, 65536]", Layout::Current),
        ] {
            assert_eq!(layout_of(format), Some(expected), "{format}");
        }
    }

    #[test]
    fn semantic_versions_follow_the_client_grammar() {
        let triple = |text| SemVersion::parse(text).map(|version| version.triple);
        assert_eq!(triple("1"), Some([1, 0, 0]));
        assert_eq!(triple("1.2"), Some([1, 2, 0]));
        assert_eq!(triple("1.2.3-beta.1+build.7"), Some([1, 2, 3]));
        for text in [
            "", "01", "1.02", "123456", "1.2.3.4", "1.2-beta", " 1", "1.2.3-01", "x",
        ] {
            assert_eq!(SemVersion::parse(text), None, "{text:?}");
        }
    }
}
