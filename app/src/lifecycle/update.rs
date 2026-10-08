//! Update availability check: the core verifies the signed manifest; the client only records the verdict.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::install_layout::InstallLayout;

const URL_ENV: &str = "CINNABAR_UPDATE_URL";
const DISABLE_ENV: &str = "CINNABAR_UPDATE_CHECK";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_RESULT_BYTES: u64 = 16 * 1024;

/// Verdict published for whatever UI surfaces updates; mirrors the core's check result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct UpdateNotice {
    pub available: bool,
    pub current: String,
    pub latest: String,
    #[serde(default)]
    pub notes_url: Option<String>,
    #[serde(default)]
    pub artifact: Option<Artifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Artifact {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Deserialize, Serialize)]
struct Stamp {
    checked_at: u64,
    #[serde(default)]
    current: String,
}

/// Manifest platform key, matching the release pipeline's artifact names.
pub(crate) fn platform_key(os: &str, arch: &str) -> Option<String> {
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        _ => return None,
    };
    matches!(os, "macos" | "windows" | "linux").then(|| format!("{os}-{arch}"))
}

/// A stamp from the future (clock moved back) counts as due rather than suppressing checks.
fn due(last_checked: Option<u64>, now: u64) -> bool {
    last_checked.is_none_or(|last| now < last || now - last >= CHECK_INTERVAL.as_secs())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn stamp_path(layout: &InstallLayout) -> PathBuf {
    layout.user_data_root.join("update/last-check.json")
}

/// Last recorded verdict, if a newer build was found.
pub(crate) fn available(layout: &InstallLayout) -> Option<UpdateNotice> {
    read_available(
        &layout.user_data_root.join("update"),
        env!("CARGO_PKG_VERSION"),
    )
}

/// Reads a cached verdict only for the running client version.
fn read_available(directory: &Path, current: &str) -> Option<UpdateNotice> {
    let bytes = fs::read(directory.join("available.json")).ok()?;
    serde_json::from_slice::<UpdateNotice>(&bytes)
        .ok()
        .filter(|notice| notice.available && notice.current == current)
}

fn manifest_url(layout: &InstallLayout) -> Option<String> {
    std::env::var(URL_ENV)
        .ok()
        .or_else(|| fs::read_to_string(layout.resource_root.join("update-url")).ok())
        .map(|url| url.trim().to_owned())
        .filter(|url| !url.is_empty())
}

/// Starts a background check at most once a day; silent when no manifest URL is configured.
pub(crate) fn check_in_background(layout: &InstallLayout) {
    if !layout.is_installed() || std::env::var(DISABLE_ENV).is_ok_and(|value| value == "0") {
        return;
    }
    let Some(url) = manifest_url(layout) else {
        return;
    };
    let Some(platform) = platform_key(std::env::consts::OS, std::env::consts::ARCH) else {
        return;
    };
    let stamp = stamp_path(layout);
    let last = fs::read(&stamp)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stamp>(&bytes).ok())
        .filter(|stamp| stamp.current == env!("CARGO_PKG_VERSION"))
        .map(|stamp| stamp.checked_at);
    if !due(last, now_secs()) {
        return;
    }
    let core = layout.core_executable.clone();
    let directory = layout.user_data_root.join("update");
    std::thread::spawn(move || {
        if let Some(notice) = run_check(&core, &url, &platform) {
            record(&directory, &notice);
        }
    });
}

fn run_check(core: &Path, url: &str, platform: &str) -> Option<UpdateNotice> {
    let child = super::children::spawn(
        Command::new(core)
            .args(["check-update", "-manifest-url", url, "-platform", platform])
            .args(["-current", env!("CARGO_PKG_VERSION")])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )
    .ok()?;
    let mut output = Vec::new();
    child
        .take_stdout()?
        .take(MAX_RESULT_BYTES)
        .read_to_end(&mut output)
        .ok()?;
    child.wait()?.success().then_some(())?;
    parse_notice(&output)
}

fn parse_notice(bytes: &[u8]) -> Option<UpdateNotice> {
    serde_json::from_slice(bytes).ok()
}

fn record(directory: &Path, notice: &UpdateNotice) {
    let _ = fs::create_dir_all(directory);
    if let Ok(bytes) = serde_json::to_vec(&Stamp {
        checked_at: now_secs(),
        current: notice.current.clone(),
    }) {
        let _ = fs::write(directory.join("last-check.json"), bytes);
    }
    let available = directory.join("available.json");
    if notice.available {
        if let Ok(bytes) = serde_json::to_vec(notice) {
            let _ = fs::write(available, bytes);
        }
    } else {
        let _ = fs::remove_file(available);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_upgrading_invalidates_cached_update_notices() {
        let dir = std::env::temp_dir().join(format!("cinnabar-old-update-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("available.json"),
            br#"{"available":true,"current":"old","latest":"new"}"#,
        )
        .unwrap();
        assert!(read_available(&dir, "old").is_some());
        let notice = read_available(&dir, "new");
        fs::remove_dir_all(dir).unwrap();
        assert!(notice.is_none());
    }

    #[test]
    fn platform_keys_match_the_release_artifact_names() {
        assert_eq!(
            platform_key("macos", "aarch64").as_deref(),
            Some("macos-arm64")
        );
        assert_eq!(
            platform_key("windows", "x86_64").as_deref(),
            Some("windows-x86_64")
        );
        assert_eq!(
            platform_key("linux", "x86_64").as_deref(),
            Some("linux-x86_64")
        );
        assert_eq!(platform_key("linux", "riscv64"), None);
        assert_eq!(platform_key("freebsd", "x86_64"), None);
    }

    #[test]
    fn checks_run_at_most_once_per_interval() {
        assert!(due(None, 10));
        assert!(!due(Some(1_000), 1_000 + 3_600));
        assert!(due(Some(1_000), 1_000 + 24 * 3_600));
        assert!(due(Some(5_000), 100));
    }

    #[test]
    fn core_result_json_parses_with_and_without_an_artifact() {
        let with = br#"{"available":true,"current":"0.1.0","latest":"0.2.0","artifact":{"url":"https://x/y","sha256":"ab","size":9}}"#;
        assert_eq!(parse_notice(with).unwrap().artifact.unwrap().size, 9);
        let without = br#"{"available":false,"current":"0.2.0","latest":"0.2.0"}"#;
        assert!(!parse_notice(without).unwrap().available);
        assert!(parse_notice(b"nope").is_none());
    }
}
