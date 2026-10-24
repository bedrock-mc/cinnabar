//! Machine-readable first-run progress: mirrored to `logs/first-run-status.json` and fed to the
//! setup window.

use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    AwaitingConsent,
    Downloading,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct Status {
    pub phase: Phase,
    pub step: usize,
    pub total: usize,
    pub label: String,
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_total: Option<u64>,
}

impl Status {
    pub(super) fn new(phase: Phase, step: usize, total: usize, label: &str) -> Self {
        Self {
            phase,
            step,
            total,
            label: label.to_owned(),
            error: None,
            downloaded: None,
            download_total: None,
        }
    }

    /// Setup is waiting on the user's consent, which no timeout should treat as a hang.
    pub(super) fn awaiting_consent() -> Self {
        Self::new(Phase::AwaitingConsent, 0, 0, "Waiting for consent")
    }

    /// Consent is resolved and preparation is starting; it carries no step count yet.
    pub(super) fn starting() -> Self {
        Self::new(Phase::Running, 0, 0, "Starting setup")
    }

    /// Every required setup stage has finished; the game can start.
    pub(super) fn ready() -> Self {
        Self::new(Phase::Done, 0, 0, "Ready")
    }

    pub(super) fn downloading(downloaded: u64, total: Option<u64>) -> Self {
        Self {
            downloaded: Some(downloaded),
            download_total: total,
            ..Self::new(Phase::Downloading, 0, 0, "Downloading Minecraft resources")
        }
    }

    /// One line for a plain progress display, with downloads in megabytes.
    pub(super) fn summary(&self) -> String {
        match (self.downloaded, self.download_total) {
            (Some(received), Some(total)) => format!(
                "{}: {} / {} MB",
                self.label,
                received / 1_000_000,
                total / 1_000_000
            ),
            (Some(received), None) => format!("{}: {} MB", self.label, received / 1_000_000),
            _ => self.label.clone(),
        }
    }

    pub(super) fn failed(label: &str, error: &str) -> Self {
        Self {
            error: Some(error.to_owned()),
            ..Self::new(Phase::Failed, 0, 0, label)
        }
    }
}

/// Whether a status file's `report` says setup is waiting on the user.
pub(super) fn awaiting_user(report: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct Report {
        phase: Phase,
    }
    serde_json::from_slice::<Report>(report)
        .is_ok_and(|report| report.phase == Phase::AwaitingConsent)
}

/// Replaces the status file atomically so readers never observe a partial document.
pub(super) fn write(path: &Path, status: &Status) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec(status)?)
        .with_context(|| format!("write {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("publish {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Dir;

    #[test]
    fn status_round_trips_as_snake_case_json() {
        let dir = Dir::new("status");
        let path = dir.path().join("logs/first-run-status.json");
        write(&path, &Status::new(Phase::AwaitingConsent, 0, 3, "x")).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["phase"], "awaiting_consent");
        assert_eq!(value["total"], 3);
        assert!(value.get("downloaded").is_none());
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn only_the_consent_prompt_reads_as_awaiting_the_user() {
        let dir = Dir::new("status-awaiting");
        let path = dir.path().join("s.json");
        write(&path, &Status::awaiting_consent()).unwrap();
        assert!(awaiting_user(&fs::read(&path).unwrap()));
        write(&path, &Status::downloading(5, Some(10))).unwrap();
        assert!(!awaiting_user(&fs::read(&path).unwrap()));
        assert!(!awaiting_user(b"not json"));
    }

    #[test]
    fn download_progress_carries_bytes() {
        let dir = Dir::new("status-download");
        let path = dir.path().join("s.json");
        write(&path, &Status::downloading(5, Some(10))).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["phase"], "downloading");
        assert_eq!(
            (
                value["downloaded"].as_u64(),
                value["download_total"].as_u64()
            ),
            (Some(5), Some(10))
        );
    }
}
