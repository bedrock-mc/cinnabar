//! Crate declarations in `[package.metadata.cinnabar]`, which the architecture gate enforces and
//! devtool reads to choose pull-request checks.

/// The `[package.metadata]` table holding the declarations.
pub const TABLE: &str = "cinnabar";

/// Key listing the platforms whose code a crate contains.
pub const PLATFORM_SPECIFIC: &str = "platform-specific";

/// Key listing repository paths outside the crate that its sources read.
pub const READS: &str = "reads";

pub const MACOS: &str = "macos";
pub const WINDOWS: &str = "windows";

/// Platforms a crate may declare; each has its own pull-request runner.
pub const PLATFORMS: [&str; 2] = [MACOS, WINDOWS];

/// Whether a declared read names `path`: the pattern matches it, or names a directory holding it.
#[must_use]
pub fn read_names(pattern: &str, path: &str) -> bool {
    wildcard_match(pattern, path) || within(path, pattern)
}

/// Whether `path` is `root` or lies below it.
#[must_use]
pub fn within(path: &str, root: &str) -> bool {
    path.strip_prefix(root)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Matches `*` against any run of characters, `/` included.
#[must_use]
pub fn wildcard_match(pattern: &str, value: &str) -> bool {
    let (pattern, value) = (pattern.as_bytes(), value.as_bytes());
    let (mut pattern_index, mut value_index) = (0, 0);
    let (mut star_index, mut star_value_index) = (None, 0);
    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
                pattern_index += 1;
            }
            star_index = Some(pattern_index);
            star_value_index = value_index;
        } else if let Some(after_star) = star_index {
            star_value_index += 1;
            value_index = star_value_index;
            pattern_index = after_star;
        } else {
            return false;
        }
    }
    pattern[pattern_index..].iter().all(|byte| *byte == b'*')
}

#[cfg(test)]
mod tests {
    use super::{read_names, wildcard_match, within};

    #[test]
    fn stars_match_across_directories() {
        assert!(wildcard_match(
            "crates/assets/data/*",
            "crates/assets/data/a/b.bin"
        ));
        assert!(wildcard_match(
            "tools/bedsimtrace*",
            "tools/bedsimtrace-v0.1.4/go.mod"
        ));
        assert!(wildcard_match("*.png", "art/sky.png"));
        assert!(!wildcard_match(
            "crates/assets/data/*",
            "crates/assets/src/lib.rs"
        ));
        assert!(!wildcard_match("plan.md", "plan.md.bak"));
    }

    #[test]
    fn directories_contain_only_their_own_descendants() {
        assert!(within("docs/evidence/ledger.md", "docs/evidence"));
        assert!(within("docs/evidence", "docs/evidence"));
        assert!(!within("docs/evidence-old/ledger.md", "docs/evidence"));
    }

    #[test]
    fn reads_name_matching_paths_and_directory_contents() {
        assert!(read_names(
            "assets/java-hud",
            "assets/java-hud/ui/hud_screen.json"
        ));
        assert!(read_names(
            "crates/protocol/fixtures/*",
            "crates/protocol/fixtures/a.bin"
        ));
        assert!(!read_names(
            "crates/protocol/fixtures/a.bin",
            "crates/protocol/fixtures"
        ));
    }
}
