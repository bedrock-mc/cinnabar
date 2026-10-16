//! The installed build's release highlights, embedded at package time so they show offline.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Changelog {
    pub version: String,
    pub published_at: String,
    pub highlights: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub url: Option<String>,
    /// True on the first launch of this version; reading it marks it seen.
    #[serde(default)]
    pub unseen: bool,
}

/// What `packaging/changelog.py highlights` produced for this build, if anything.
const EMBEDDED: &str = include_str!(concat!(env!("OUT_DIR"), "/changelog.json"));

#[derive(Default, Deserialize, Serialize)]
struct Seen {
    version: String,
}

/// The changelog for `version` from `embedded`, marking it seen in `seen_file`.
pub fn take(embedded: &str, version: &str, seen_file: &Path) -> Option<Changelog> {
    let mut changelog: Changelog = serde_json::from_str::<Option<Changelog>>(embedded).ok()??;
    if changelog.version != version {
        return None;
    }
    let seen: Seen = std::fs::read(seen_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    changelog.unseen = seen.version != version;
    if changelog.unseen {
        let saved = serde_json::to_vec(&Seen {
            version: version.to_owned(),
        })
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(seen_file, bytes));
        if let Err(error) = saved {
            tracing::debug!("could not record the seen changelog: {error}");
        }
    }
    Some(changelog)
}

/// This build's changelog.
pub fn installed(version: &str, seen_file: &Path) -> Option<Changelog> {
    take(EMBEDDED, version, seen_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_version_is_unseen_once_and_other_builds_have_none() {
        let root = tempfile::tempdir().unwrap();
        let seen = root.path().join("seen.json");
        let embedded = r#"{"version":"1.2.0","publishedAt":"2026-10-16T00:00:00Z","highlights":["Add hats"],"url":"https://example.test/v1.2.0"}"#;
        assert!(take(embedded, "1.2.0", &seen).unwrap().unseen);
        let again = take(embedded, "1.2.0", &seen).unwrap();
        assert!(!again.unseen);
        assert_eq!(again.highlights, ["Add hats"]);
        assert_eq!(take(embedded, "1.3.0", &seen), None);
        assert_eq!(take("null", "1.2.0", &seen), None);
    }
}
