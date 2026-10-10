//! Update availability check: fetches the signed release manifest, verifies it against the
//! trusted keys baked into this build, and records the verdict at most daily.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::install_layout::InstallLayout;

const URL_ENV: &str = "CINNABAR_UPDATE_URL";
const DISABLE_ENV: &str = "CINNABAR_UPDATE_CHECK";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const CHANNEL: &str = "stable";
/// `id:base64[,...]` public keys trusted for manifests, set at build time by `UPDATE_TRUSTED_KEYS`.
const TRUSTED_KEYS: &str = match option_env!("UPDATE_TRUSTED_KEYS") {
    Some(keys) => keys,
    None => "",
};

/// Verdict published for whatever UI surfaces updates.
pub(crate) type UpdateNotice = update_manifest::Verdict;

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

/// Starts a background check at most once a day; silent without a manifest URL or trusted keys.
pub(crate) fn check_in_background(layout: &InstallLayout) {
    if !layout.is_installed() || std::env::var(DISABLE_ENV).is_ok_and(|value| value == "0") {
        return;
    }
    let Some(keys) = update_manifest::parse_keys(TRUSTED_KEYS)
        .ok()
        .filter(|keys| !keys.is_empty())
    else {
        return;
    };
    let Some(url) = manifest_url(layout).filter(|url| https(url)) else {
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
    let directory = layout.user_data_root.join("update");
    std::thread::spawn(move || {
        let verdict = fetch(&url).and_then(|body| {
            update_manifest::check(&body, &keys, CHANNEL, &platform, env!("CARGO_PKG_VERSION")).ok()
        });
        if let Some(notice) = verdict {
            record(&directory, &notice);
        }
    });
}

/// Manifests are only ever fetched over HTTPS.
fn https(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some())
}

/// The served envelope, refused when it is not a 200 or exceeds the envelope bound.
fn fetch(url: &str) -> Option<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .ok()?;
    runtime.block_on(async {
        let mut response = client.get(url).send().await.ok()?;
        if response.status() != reqwest::StatusCode::OK {
            return None;
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if body.len() + chunk.len() > update_manifest::MAX_ENVELOPE_BYTES {
                return None;
            }
            body.extend_from_slice(&chunk);
        }
        Some(body)
    })
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
    fn recorded_verdicts_parse_with_and_without_an_artifact() {
        let with = br#"{"available":true,"current":"0.1.0","latest":"0.2.0","artifact":{"url":"https://x/y","sha256":"ab","size":9}}"#;
        let notice: UpdateNotice = serde_json::from_slice(with).unwrap();
        assert_eq!(notice.artifact.unwrap().size, 9);
        let without = br#"{"available":false,"current":"0.2.0","latest":"0.2.0"}"#;
        assert!(
            !serde_json::from_slice::<UpdateNotice>(without)
                .unwrap()
                .available
        );
        assert!(serde_json::from_slice::<UpdateNotice>(b"nope").is_err());
    }

    #[test]
    fn manifests_are_fetched_only_over_https() {
        assert!(https("https://example.test/update-stable.json"));
        for url in ["http://example.test/m", "file:///m", "not a url", ""] {
            assert!(!https(url), "{url} accepted");
        }
    }
}
