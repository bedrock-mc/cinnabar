use serde::Serialize;

pub(crate) const MAX_NAME_CHARS: usize = 64;

/// Why a local-world request or server start failed.
#[derive(Debug, thiserror::Error)]
pub enum WorldError {
    #[error("world not found")]
    NotFound,
    #[error("{}", launcher::local_worlds::generators::GENERATOR_CATALOG_ERROR)]
    GeneratorCatalog,
    #[error("{}", launcher::local_worlds::generators::LOADING_GENERATORS)]
    GeneratorsPending,
    #[error("This world's generator isn't available in the current local server")]
    GeneratorUnavailable,
    #[error("this server does not support world generator {0:?}; select a server that provides it")]
    MissingGenerator(String),
    #[error("invalid world settings: {0}")]
    Invalid(String),
    #[error("another world is starting, running or stopping")]
    Busy,
    #[error("world is open")]
    InUse,
    #[error("no world is open")]
    NotOpen,
    #[error("the Minecraft EULA must be accepted before the server is downloaded")]
    EulaRequired,
    #[error("world backend is not available on this platform")]
    BackendUnavailable,
    #[error("still checking whether Docker is running; try again")]
    RuntimePending,
    #[error("Docker is not running")]
    DockerNotRunning,
    /// An image without a digest drifts from the joinable protocol.
    #[error("the server container image is not pinned to a digest")]
    ImageNotPinned,
    #[error("the start was cancelled")]
    Cancelled,
    /// An archive was refused for its contents; the message names no local path.
    #[error("{0}")]
    Rejected(String),
    /// Detail for the log; it may name local paths.
    #[error("{0}")]
    Failed(String),
}

impl WorldError {
    /// The message the menu shows; failures that may name local paths stay generic.
    #[must_use]
    pub fn public_message(&self) -> String {
        match self {
            Self::Failed(_) => "world operation failed".to_owned(),
            known => known.to_string(),
        }
    }

    /// The open world's path-free failure reason for its status.
    pub(crate) fn status_text(&self) -> String {
        match self {
            Self::Failed(_) | Self::Rejected(_) | Self::Cancelled => {
                "local world server failed to start".to_owned()
            }
            known => known.to_string(),
        }
    }
}

/// A [`WorldError::Failed`] with `detail`.
pub(crate) fn failed(detail: impl Into<String>) -> WorldError {
    WorldError::Failed(detail.into())
}

/// The trimmed world name, or why it cannot be saved.
pub(crate) fn validate_name(name: &str) -> Result<String, WorldError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(WorldError::Invalid("name is empty".to_owned()));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(WorldError::Invalid(format!(
            "name must be at most {MAX_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(WorldError::Invalid(
            "name contains control characters".to_owned(),
        ));
    }
    Ok(name.to_owned())
}

/// The value's serde name, as saved worlds and server settings spell it.
pub(crate) fn wire_name(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(validate_name("  My World ").unwrap(), "My World");
        for bad in [
            String::new(),
            "  ".to_owned(),
            "bad\0name".to_owned(),
            "x".repeat(MAX_NAME_CHARS + 1),
        ] {
            assert!(
                matches!(validate_name(&bad), Err(WorldError::Invalid(_))),
                "{bad:?}"
            );
        }
        assert!(validate_name(&"é".repeat(MAX_NAME_CHARS)).is_ok());
    }

    #[test]
    fn public_messages_never_carry_failure_detail() {
        let failure = failed("open /Users/secret/worlds: denied");
        assert_eq!(failure.public_message(), "world operation failed");
        assert_eq!(failure.status_text(), "local world server failed to start");
        assert_eq!(WorldError::NotFound.public_message(), "world not found");
        assert_eq!(
            WorldError::DockerNotRunning.status_text(),
            "Docker is not running"
        );
        assert!(
            WorldError::Invalid("name is empty".into())
                .public_message()
                .contains("name is empty")
        );
    }
}
