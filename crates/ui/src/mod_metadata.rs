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
    /// Author opt-in to automatic installation of a named latest-release WASM asset.
    pub update_url: Option<GitHubModAsset>,
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

/// A stable GitHub latest-release download URL naming exactly one WASM asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubModAsset {
    repository: GitHubRepository,
    asset: String,
}

impl std::str::FromStr for GitHubModAsset {
    type Err = &'static str;

    /// Parses the same exact-asset URL contract used by component declarations.
    fn from_str(url: &str) -> Result<Self, Self::Err> {
        Self::parse(url).ok_or("expected a GitHub latest-release WASM download URL")
    }
}

impl GitHubModAsset {
    /// Accepts only `https://github.com/OWNER/REPO/releases/latest/download/FILE.wasm`.
    pub fn parse(url: &str) -> Option<Self> {
        if url.len() > 512 {
            return None;
        }
        let (repository, asset) = url.split_once("/releases/latest/download/")?;
        let repository = GitHubRepository::parse(repository)?;
        if asset.len() > 128
            || !asset.ends_with(".wasm")
            || asset.starts_with('.')
            || !asset
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return None;
        }
        Some(Self {
            repository,
            asset: asset.into(),
        })
    }

    /// The repository containing the named release asset.
    pub fn repository(&self) -> &GitHubRepository {
        &self.repository
    }

    /// The case-sensitive release asset name.
    pub fn asset(&self) -> &str {
        &self.asset
    }
}

#[cfg(test)]
mod update_url_tests {
    use super::*;

    #[test]
    fn update_urls_name_exact_latest_release_wasm_assets() {
        let source = GitHubModAsset::parse(
            "https://github.com/Example/Zoom/releases/latest/download/Zoom.wasm",
        )
        .unwrap();
        assert_eq!(source.repository().slug(), "example/zoom");
        assert_eq!(source.asset(), "Zoom.wasm");
        for path in [
            "zoom.wasm?token=x",
            "../zoom.wasm",
            "zoom.zip",
            "zoom%2ewasm",
            ".hidden.wasm",
            "zoom.wasm#fragment",
            "zoom.wasm/",
            "zoom.wasm\n",
        ] {
            assert!(
                GitHubModAsset::parse(&format!(
                    "https://github.com/example/zoom/releases/latest/download/{path}"
                ))
                .is_none(),
                "{path}"
            );
        }
        assert!(GitHubModAsset::parse("https://example.com/mod.wasm").is_none());
        assert!(
            GitHubModAsset::parse("https://github.com/example/zoom/releases/download/v1/zoom.wasm")
                .is_none()
        );
    }
}
