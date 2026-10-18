//! What a personal mod component says about itself, for its Mods library card.

use std::sync::Arc;

/// Maximum bytes in a mod's displayed installed or available release version.
pub const MAX_VERSION_BYTES: usize = 32;

/// Bounded single-line text from the component's standard metadata, plus a validated icon.
/// Absent fields fall back to the file name or a placeholder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    /// The standard section's freeform contact text.
    pub authors: Option<String>,
    pub licenses: Option<String>,
    /// Author opt-in to launch-time checks of this public GitHub repository's releases.
    pub update_repository: Option<GitHubRepository>,
    /// Encoded PNG source that decoded within the icon section's limits.
    pub icon: Option<Arc<[u8]>>,
}

/// A public GitHub repository, excluding credentials, queries and arbitrary URL paths.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GitHubRepository(String);

impl std::str::FromStr for GitHubRepository {
    type Err = &'static str;

    /// Parses the same restricted HTTPS repository URLs used by component metadata.
    fn from_str(url: &str) -> Result<Self, Self::Err> {
        Self::parse(url).ok_or("expected an HTTPS GitHub repository URL")
    }
}

impl GitHubRepository {
    /// Accepts an HTTPS repository URL, optionally ending in `.git` or a slash.
    pub fn parse(url: &str) -> Option<Self> {
        if url.len() > 256 {
            return None;
        }
        let path = url.strip_prefix("https://github.com/")?;
        let path = path.strip_suffix('/').unwrap_or(path);
        let (owner, name) = path.split_once('/')?;
        let name = name.strip_suffix(".git").unwrap_or(name);
        let valid = |value: &str, repository: bool| {
            !value.is_empty()
                && value != "."
                && value != ".."
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || byte == b'-'
                        || (repository && matches!(byte, b'_' | b'.'))
                })
        };
        (valid(owner, false) && valid(name, true))
            .then(|| Self(format!("{owner}/{name}").to_ascii_lowercase()))
    }

    /// The validated owner and repository used to construct GitHub API paths.
    pub fn slug(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_urls_are_optional_and_restricted_to_github_repositories() {
        for suffix in ["", "/", ".git", ".git/"] {
            let repo = GitHubRepository::parse(&format!("https://github.com/Example/Zoom{suffix}"))
                .unwrap();
            assert_eq!(repo.slug(), "example/zoom");
        }
        for url in [
            "http://github.com/example/zoom",
            "https://github.com.evil.test/example/zoom",
            "https://github.com@example.test/example/zoom",
            "https://github.com/example/zoom/releases",
            "https://github.com/example/zoom?token=abc",
            "https://github.com/example/zoom#fragment",
            "https://github.com/example/..",
            "https://github.com/example/%2fzoom",
            "https://github.com/example/zoom\n",
        ] {
            assert!(GitHubRepository::parse(url).is_none(), "{url}");
        }
    }
}
