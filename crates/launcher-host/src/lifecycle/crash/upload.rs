//! Opted-in crash report uploads to the Cinnabar API.
//!
//! Nothing is sent without consent: the player's Send report on one crash, or the "Send crash
//! reports automatically" setting. [`DISABLE_ENV`] `0` turns both off. Desktop crash monitors
//! and Android's next launch send; the crashing process never does. Each report is stored at most
//! once by the API; retries keep its random report id (see [`crash_report::upload::state`]).

use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crash_report::upload::{
    self, Ids,
    state::{self, Consent, Outcome, Upload},
};
use launcher::{
    install_layout::InstallLayout,
    menu::settings_options::{SETTINGS_FILE, SettingsOptions},
};

pub mod previous_exit;

/// `0`, `false` or `off` disables crash uploads, overriding consent of either kind.
pub const DISABLE_ENV: &str = "CINNABAR_CRASH_UPLOAD";
/// Replaces the API base URL, for a local API; plain HTTP only reaches loopback hosts.
pub const URL_ENV: &str = "CINNABAR_CRASH_UPLOAD_URL";
/// The API route that stores crash reports.
pub const PATH: &str = "/v1/crash-reports";
/// The random per-install id, under the user data root.
const INSTALL_ID_FILE: &str = "crash-upload-id";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// True when [`DISABLE_ENV`] turns uploads off.
pub fn disabled() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        )
    })
}

/// What uploads may do, read once from the environment and settings.
#[derive(Clone, Debug)]
pub struct Policy {
    pub endpoint: url::Url,
    /// The player turned on "Send crash reports automatically".
    pub automatic: bool,
}

impl Policy {
    /// The policy for `layout`; `None` when [`DISABLE_ENV`] disables uploads or [`URL_ENV`] is
    /// not a usable endpoint.
    pub fn load(layout: &InstallLayout) -> Option<Self> {
        if disabled() {
            return None;
        }
        let base = std::env::var(URL_ENV)
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| bridge::contract::CINNABAR_API_BASE_URL.to_owned());
        Some(Self {
            endpoint: endpoint(&base)?,
            automatic: SettingsOptions::load(&layout.user_config_root.join(SETTINGS_FILE))
                .send_crash_reports(),
        })
    }
}

/// Records the player's or the setting's consent for `folder` under a fresh report id.
pub fn consent(folder: &Path, consent: Consent, now_ms: u64) -> io::Result<Upload> {
    state::consent(folder, consent, &random_id()?, now_ms)
}

/// Records automatic consent after the producer has finished every member of `folder`.
/// Unmarked folders are never picked up by a sender while another process may be writing them.
pub fn completed(layout: &InstallLayout, folder: &Path) {
    completed_with_settings(
        folder,
        &layout.user_config_root.join(SETTINGS_FILE),
        now_ms(),
    );
}

/// Records completion from a healthy report producer using its settings path and supplied time.
pub(super) fn completed_with_settings(folder: &Path, settings: &Path, now_ms: u64) {
    if !disabled()
        && SettingsOptions::load(settings).send_crash_reports()
        && let Err(error) = consent(folder, Consent::Auto, now_ms)
    {
        tracing::warn!("crash report consent not recorded: {error}");
    }
}

/// Sends already-consented due folders once, newest first. The report's producer records
/// automatic consent only after its logs and native metadata are complete.
pub fn send_due(layout: &InstallLayout, policy: &Policy) -> Vec<(PathBuf, io::Result<Upload>)> {
    if disabled() {
        return Vec::new();
    }
    let crash_dir = layout.crash_dir();
    if policy.automatic {
        recover_completed(layout, &crash_dir);
    }
    state::due(&crash_dir, now_ms())
        .into_iter()
        .map(|folder| {
            let result = send_inner(layout, &folder, policy, true);
            (folder, result)
        })
        .collect()
}

/// Recovers finished in-process fallbacks; unmarked folders may still be under construction.
/// Producers create the marker last, and current settings remain the authority for consent.
fn recover_completed(layout: &InstallLayout, crash_dir: &Path) {
    for folder in crash_report::folder::list(crash_dir) {
        if state::read(&folder).is_none()
            && folder
                .join(crash_report::folder::FALLBACK_READY_FILE)
                .is_file()
        {
            completed(layout, &folder);
        }
    }
}

/// Runs [`send_due`] on its own thread, so a monitor or launch never waits on the network;
/// `None` when uploads are disabled.
pub fn spawn_send_due(layout: InstallLayout) -> Option<std::thread::JoinHandle<()>> {
    let policy = Policy::load(&layout)?;
    std::thread::Builder::new()
        .name("crash-upload".into())
        .spawn(move || {
            for (folder, result) in send_due(&layout, &policy) {
                log_result(&folder, &result);
            }
        })
        .map_err(|error| tracing::warn!("crash upload thread did not start: {error}"))
        .ok()
}

/// Logs what an attempt did, without the folder's full path.
pub fn log_result(folder: &Path, result: &io::Result<Upload>) {
    let name = folder
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    match result {
        Ok(Upload::Sent { .. }) => tracing::info!("crash report {name} sent"),
        Ok(Upload::Pending { attempts, .. }) => {
            tracing::info!("crash report {name} not sent yet (attempt {attempts}); will retry");
        }
        Ok(Upload::Abandoned { reason, .. }) => {
            tracing::warn!("crash report {name} not sent: {reason}")
        }
        Err(error) => tracing::warn!("crash report {name} not sent: {error}"),
    }
}

/// Sends one consented folder now, whatever its schedule; a sent or unconsented folder keeps
/// its state.
pub fn send(layout: &InstallLayout, folder: &Path, policy: &Policy) -> io::Result<Upload> {
    send_inner(layout, folder, policy, false)
}

/// Claims one report and sends it, checking its persisted schedule for background workers.
fn send_inner(
    layout: &InstallLayout,
    folder: &Path,
    policy: &Policy,
    only_due: bool,
) -> io::Result<Upload> {
    if disabled() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "crash uploads are disabled",
        ));
    }
    let Some(_claim) = state::claim(folder, now_ms())? else {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "another process is sending this crash report",
        ));
    };
    // Read after taking the lock: a sender that just finished may have marked it sent.
    let pending = state::read(folder).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "no consent to send this crash report",
        )
    })?;
    let Upload::Pending {
        report_id,
        consent,
        next_at_ms,
        ..
    } = &pending
    else {
        return Ok(pending);
    };
    if only_due && *next_at_ms > now_ms() {
        return Ok(pending);
    }
    authorize(layout, policy, *consent)?;
    let install = install_id(&layout.user_data_root)?;
    let body = upload::build(
        folder,
        Ids {
            install: &install,
            report: report_id,
        },
        home().as_deref(),
    )
    .and_then(upload::encode);
    let outcome = match body {
        Ok(body) => {
            authorize(layout, policy, *consent)?;
            post(&policy.endpoint, body)
        }
        Err(error) => Outcome::Rejected(format!("unreadable crash report: {error}")),
    };
    state::record(folder, &outcome, now_ms())
}

/// Rechecks the live setting so a cached worker cannot keep automatic consent after opt-out.
fn authorize(layout: &InstallLayout, policy: &Policy, consent: Consent) -> io::Result<()> {
    if disabled()
        || (consent == Consent::Auto
            && (!policy.automatic
                || !SettingsOptions::load(&layout.user_config_root.join(SETTINGS_FILE))
                    .send_crash_reports()))
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "crash upload consent is disabled",
        ));
    }
    Ok(())
}

/// `base` joined with [`PATH`]; HTTPS, or HTTP to a loopback host only.
fn endpoint(base: &str) -> Option<url::Url> {
    let url = url::Url::parse(base).ok()?.join(PATH).ok()?;
    let loopback = match url.host()? {
        url::Host::Domain(domain) => domain == "localhost",
        url::Host::Ipv4(address) => address.is_loopback(),
        url::Host::Ipv6(address) => address.is_loopback(),
    };
    (url.scheme() == "https" || (url.scheme() == "http" && loopback)).then_some(url)
}

/// Posts one gzip payload and classifies the answer.
fn post(endpoint: &url::Url, body: Vec<u8>) -> Outcome {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return Outcome::Retry { after_ms: None },
    };
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .https_only(endpoint.scheme() == "https")
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build();
        let Ok(client) = client else {
            return Outcome::Retry { after_ms: None };
        };
        let response = client
            .post(endpoint.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::CONTENT_ENCODING, "gzip")
            .body(body)
            .send()
            .await;
        match response {
            Ok(response) => {
                let retry_after = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .map(|seconds| seconds.saturating_mul(1000));
                classify(response.status().as_u16(), retry_after)
            }
            Err(_) => Outcome::Retry { after_ms: None },
        }
    })
}

/// What an HTTP status means for a report: stored (or already stored), never storable, or
/// worth retrying.
fn classify(status: u16, retry_after_ms: Option<u64>) -> Outcome {
    match status {
        200..=299 => Outcome::Sent,
        400 | 409 | 413 | 415 | 422 => {
            Outcome::Rejected(format!("the server refused it (HTTP {status})"))
        }
        _ => Outcome::Retry {
            after_ms: retry_after_ms,
        },
    }
}

/// This install's random upload id, created on first use; it identifies no player or device.
pub fn install_id(data_root: &Path) -> io::Result<String> {
    let path = data_root.join(INSTALL_ID_FILE);
    if let Ok(text) = std::fs::read_to_string(&path)
        && upload::valid_id(text.trim())
    {
        return Ok(text.trim().to_owned());
    }
    std::fs::create_dir_all(data_root)?;
    let id = random_id()?;
    let mut temporary = tempfile::NamedTempFile::new_in(data_root)?;
    io::Write::write_all(&mut temporary, id.as_bytes())?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(id),
        // Another process created it first; use theirs if it is valid.
        Err(error) => match std::fs::read_to_string(&path) {
            Ok(text) if upload::valid_id(text.trim()) => Ok(text.trim().to_owned()),
            _ => {
                error.file.persist(&path).map_err(|error| error.error)?;
                Ok(id)
            }
        },
    }
}

/// 128 OS-random bits as lowercase hex.
fn random_id() -> io::Result<String> {
    let mut bytes = [0_u8; upload::ID_LEN / 2];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The player's home directory, which payloads never include.
fn home() -> Option<String> {
    std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok()
        .filter(|home| !home.is_empty())
}

/// Unix milliseconds now; uploads are edge code that reads the clock.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests;
