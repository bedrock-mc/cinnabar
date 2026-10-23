//! The level base game version StartGame carries, as the client resolves it.

/// A level's effective base game version. Version gates compare against it;
/// [`Self::Any`] satisfies every gate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaseGameVersion {
    /// `*`, or a missing or unparseable version, which the client's level data
    /// replaces with "any version".
    #[default]
    Any,
    /// `major.minor.patch`; a missing minor or patch reads as zero, and
    /// pre-release and build suffixes are discarded.
    Version([u16; 3]),
}

/// The version the client substitutes for the literal `beta`.
const BETA_VERSION: [u16; 3] = [9999, 9999, 9999];

impl BaseGameVersion {
    /// Parses StartGame's base game version string. Besides `*` and `beta`,
    /// the text must be a complete semantic version: `major[.minor[.patch
    /// [-pre][+build]]]`, each number `0` or one to five digits without a
    /// leading zero that fits in 16 bits. Anything else resolves to any version.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        match text {
            "*" => Self::Any,
            "beta" => Self::Version(BETA_VERSION),
            _ => parse_semantic(text).map_or(Self::Any, Self::Version),
        }
    }

    /// The parsed version, or `None` for any version.
    #[must_use]
    pub const fn version(self) -> Option<[u16; 3]> {
        match self {
            Self::Any => None,
            Self::Version(version) => Some(version),
        }
    }
}

/// Reads a complete `major[.minor[.patch[-pre][+build]]]` string.
fn parse_semantic(text: &str) -> Option<[u16; 3]> {
    let mut rest = text;
    let mut version = [0; 3];
    for (index, slot) in version.iter_mut().enumerate() {
        if index > 0 {
            match rest.strip_prefix('.') {
                Some(after) => rest = after,
                None => return rest.is_empty().then_some(version),
            }
        }
        let (value, after) = number(rest)?;
        *slot = value;
        rest = after;
    }
    if let Some(after) = rest.strip_prefix('-') {
        rest = dotted(after, prerelease_identifier)?;
    }
    if let Some(after) = rest.strip_prefix('+') {
        rest = dotted(after, build_identifier)?;
    }
    rest.is_empty().then_some(version)
}

/// Reads `0` or a one-to-five-digit number without a leading zero that fits in 16 bits.
fn number(text: &str) -> Option<(u16, &str)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let length = if text.starts_with('0') {
        1
    } else {
        digits.min(5)
    };
    if digits == 0 {
        return None;
    }
    let value = text[..length].parse().ok()?;
    Some((value, &text[length..]))
}

/// Reads one or more `identifier` matches separated by dots, returning the remainder.
fn dotted(text: &str, identifier: fn(&str) -> Option<usize>) -> Option<&str> {
    let mut rest = &text[identifier(text)?..];
    while let Some(after) = rest.strip_prefix('.') {
        let Some(length) = identifier(after) else {
            break;
        };
        rest = &after[length..];
    }
    Some(rest)
}

/// Accepts ASCII letters, digits and hyphens in a version suffix identifier.
fn is_identifier_byte(byte: &u8) -> bool {
    byte.is_ascii_alphanumeric() || *byte == b'-'
}

/// Length of a pre-release identifier: alphanumerics and hyphens, where a
/// leading zero is only allowed alone or when a non-digit follows its digits.
fn prerelease_identifier(text: &str) -> Option<usize> {
    let length = text.bytes().take_while(is_identifier_byte).count();
    let identifier = &text.as_bytes()[..length];
    match identifier.first()? {
        b'0' => {
            let digits = identifier.iter().take_while(|b| b.is_ascii_digit()).count();
            if digits < length {
                Some(length)
            } else {
                Some(1)
            }
        }
        _ => Some(length),
    }
}

/// Length of a non-empty build identifier of alphanumerics and hyphens.
fn build_identifier(text: &str) -> Option<usize> {
    let length = text.bytes().take_while(is_identifier_byte).count();
    (length > 0).then_some(length)
}

#[cfg(test)]
mod tests {
    use super::BaseGameVersion::{self, Any, Version};

    #[test]
    fn complete_versions_parse_and_drop_suffixes() {
        for (text, expected) in [
            ("1.20.10", Version([1, 20, 10])),
            ("1.19.80", Version([1, 19, 80])),
            ("1.20", Version([1, 20, 0])),
            ("1", Version([1, 0, 0])),
            ("0.0.0", Version([0, 0, 0])),
            ("1.20.10-beta.1+build.7", Version([1, 20, 10])),
            ("1.20.10-0", Version([1, 20, 10])),
            ("1.20.10-0a", Version([1, 20, 10])),
            ("1.20.10--", Version([1, 20, 10])),
            ("65535.0.0", Version([65535, 0, 0])),
            ("beta", Version([9999, 9999, 9999])),
            ("*", Any),
        ] {
            assert_eq!(BaseGameVersion::parse(text), expected, "{text}");
        }
    }

    /// The client keeps any version for text its semantic-version parser rejects.
    #[test]
    fn malformed_versions_resolve_to_any() {
        for text in [
            "",
            " 1.20.10",
            "1.20.10 ",
            "1.",
            "1.20.",
            "01.20.10",
            "1.020.10",
            "1.20-beta",
            "1.20.10-",
            "1.20.10-01",
            "1.20.10+",
            "1.20.10.4",
            "123456",
            "65536.0.0",
            "**",
            "Beta",
            "v1.20.10",
        ] {
            assert_eq!(BaseGameVersion::parse(text), Any, "{text:?}");
        }
    }

    #[test]
    fn any_version_has_no_number() {
        assert_eq!(Any.version(), None);
        assert_eq!(Version([1, 20, 10]).version(), Some([1, 20, 10]));
    }
}
