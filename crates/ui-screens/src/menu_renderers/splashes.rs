//! Layered title text prepared before the menu chooses a line.

use serde_json::Value;

pub const MAX_SPLASH_SOURCE_BYTES: u64 = 256 * 1024;
const MAX_SPLASH_LINES: usize = 8192;

/// One resource layer: it replaces earlier lines unless `canMerge` is true.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplashSource {
    can_merge: bool,
    lines: Vec<String>,
}

impl SplashSource {
    /// Extracts a bounded plain-text list; malformed lists leave lower resources intact.
    pub fn from_json(root: &Value) -> Option<Self> {
        let lines = root.get("splashes")?.as_array()?;
        if lines.len() > MAX_SPLASH_LINES {
            return None;
        }
        Some(Self {
            can_merge: root
                .get("canMerge")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            lines: lines
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        })
    }
}

/// Selects from layers in ascending priority with deterministic caller-provided entropy.
pub fn select_splash<'a>(
    sources: impl IntoIterator<Item = &'a SplashSource>,
    seed: usize,
) -> Option<&'a str> {
    let mut lines = Vec::new();
    for source in sources {
        if !source.can_merge {
            lines.clear();
        }
        lines.extend(source.lines.iter().map(String::as_str));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines[seed % lines.len()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds original title text with an explicit resource merge policy.
    fn source(lines: &[&str], merge: bool) -> SplashSource {
        SplashSource::from_json(&serde_json::json!({"splashes": lines, "canMerge": merge})).unwrap()
    }

    #[test]
    fn upper_lists_replace_by_default_and_removal_restores_base() {
        let base = source(&["base first", "base second"], false);
        let upper = SplashSource::from_json(&serde_json::json!({"splashes": ["upper"]})).unwrap();
        assert_eq!(select_splash([&base, &upper], 1), Some("upper"));
        assert_eq!(select_splash([&base], 1), Some("base second"));
    }

    #[test]
    fn merging_lists_preserve_source_order_and_duplicate_weights() {
        let base = source(&["base"], false);
        let upper = source(&["upper", "upper"], true);
        assert_eq!(select_splash([&base, &upper], 0), Some("base"));
        assert_eq!(select_splash([&base, &upper], 1), Some("upper"));
        assert_eq!(select_splash([&base, &upper], 2), Some("upper"));
        assert_eq!(select_splash([&base, &upper], 3), Some("base"));
    }

    #[test]
    fn empty_replacements_clear_lower_lines_and_invalid_lists_are_skipped() {
        let base = source(&["base"], false);
        assert_eq!(select_splash([&base, &source(&[], false)], 0), None);
        assert!(SplashSource::from_json(&serde_json::json!({"splashes": 7})).is_none());
        assert!(
            SplashSource::from_json(
                &serde_json::json!({"splashes": vec!["x"; MAX_SPLASH_LINES + 1]})
            )
            .is_none()
        );
    }
}
