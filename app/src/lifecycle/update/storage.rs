//! Per-user preferences and advisory cache. The core re-verifies all staged data before applying it.

use super::{Ready, Status};
use crate::install_layout::InstallLayout;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const CHECK_INTERVAL: u64 = 24 * 60 * 60;

#[derive(Deserialize, Serialize)]
struct Preferences {
    enabled: bool,
}

#[derive(Deserialize, Serialize)]
struct Stamp {
    checked_at: u64,
    current: String,
}

/// Resolves the writable update cache outside the installed application.
pub(super) fn directory(layout: &InstallLayout) -> PathBuf {
    layout.user_data_root.join("update")
}

/// Accepts only a direct staging directory made by the core, never a path outside the cache.
pub(super) fn is_stage(layout: &InstallLayout, stage: &Path) -> bool {
    stage.parent() == Some(directory(layout).as_path())
        && stage
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("update-"))
        && fs::symlink_metadata(stage).is_ok_and(|info| info.file_type().is_dir())
}

/// Reads the opt-out without interpreting malformed settings as permission to download.
pub(super) fn enabled(layout: &InstallLayout) -> bool {
    match fs::read(layout.user_config_root.join("updater.json")) {
        Ok(bytes) => serde_json::from_slice::<Preferences>(&bytes).is_ok_and(|value| value.enabled),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

/// Saves a complete preference file before changing the live setting.
pub(super) fn save_enabled(layout: &InstallLayout, enabled: bool) -> io::Result<()> {
    fs::create_dir_all(&layout.user_config_root)?;
    fs::write(
        layout.user_config_root.join("updater.json"),
        serde_json::to_vec(&Preferences { enabled })?,
    )
}

/// Restores only advisory ready state belonging to this running version.
pub(super) fn restore(layout: &InstallLayout) -> Status {
    let root = directory(layout);
    if let Ok(error) = fs::read_to_string(root.join("error.txt")) {
        return Status::Error(error.chars().take(240).collect());
    }
    let ready = fs::read(root.join("ready.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Ready>(&bytes).ok())
        .filter(|ready| {
            ready.current == env!("CARGO_PKG_VERSION") && is_stage(layout, &ready.stage)
        });
    match ready {
        Some(ready) => {
            if let Ok(error) = fs::read_to_string(ready.stage.join("apply-error.txt")) {
                Status::Error(error.chars().take(240).collect())
            } else {
                Status::Ready(ready)
            }
        }
        None => Status::Idle,
    }
}

/// A successful check is throttled; errors and retained ready updates can be retried.
pub(super) fn check_due(layout: &InstallLayout) -> bool {
    let last = fs::read(directory(layout).join("last-check.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stamp>(&bytes).ok())
        .filter(|stamp| stamp.current == env!("CARGO_PKG_VERSION"))
        .map(|stamp| stamp.checked_at);
    due(last, now_secs())
}

/// Treats a backward clock change as due rather than delaying checks indefinitely.
fn due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|last| now < last || now - last >= CHECK_INTERVAL)
}

/// Records successful checks and ready state; these files never authorize installation themselves.
pub(super) fn record(layout: &InstallLayout, ready: Option<&Ready>) -> io::Result<()> {
    let root = directory(layout);
    fs::create_dir_all(&root)?;
    let stamp = Stamp {
        checked_at: now_secs(),
        current: env!("CARGO_PKG_VERSION").into(),
    };
    fs::write(root.join("last-check.json"), serde_json::to_vec(&stamp)?)?;
    let _ = fs::remove_file(root.join("error.txt"));
    if let Some(ready) = ready {
        fs::write(root.join("ready.json"), serde_json::to_vec(ready)?)?;
        let _ = fs::remove_file(ready.stage.join("apply-error.txt"));
    } else {
        let _ = fs::remove_file(root.join("ready.json"));
    }
    Ok(())
}

/// Persists an apply error so the next launcher offers a retry.
pub(super) fn record_error(layout: &InstallLayout, error: &str) {
    let root = directory(layout);
    let _ = fs::create_dir_all(&root);
    let _ = fs::write(root.join("error.txt"), error);
}

/// Returns Unix seconds for the daily-check stamp.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// Reads the installed feed URL, allowing an explicit HTTPS endpoint override.
pub(super) fn manifest_url(layout: &InstallLayout) -> Option<String> {
    std::env::var("CINNABAR_UPDATE_URL")
        .ok()
        .or_else(|| fs::read_to_string(layout.resource_root.join("update-url")).ok())
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
}

/// Reads a bounded diagnostic log without copying arbitrary large files into the menu.
pub(super) fn diagnostic(path: &Path) -> String {
    use std::io::Read;
    let mut bytes = Vec::new();
    if let Ok(file) = fs::File::open(path) {
        let _ = file.take(1024).read_to_end(&mut bytes);
    }
    let message = String::from_utf8_lossy(&bytes);
    let message = message.trim();
    if message.is_empty() {
        "Could not download or verify the update. Retry when connected.".into()
    } else {
        message.chars().take(240).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daily_stamp_allows_clock_rollback() {
        assert!(due(None, 10));
        assert!(!due(Some(1_000), 4_600));
        assert!(due(Some(1_000), 1_000 + CHECK_INTERVAL));
        assert!(due(Some(5_000), 100));
    }
}
