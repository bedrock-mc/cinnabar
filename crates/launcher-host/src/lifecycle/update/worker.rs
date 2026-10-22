//! The existing HTTPS check streams one digest-named partial file on its background thread.
use super::releases::{cache_path, read_cached};
use super::*;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use time::OffsetDateTime;
use update_installer::stage::{self, Download, Selection};
use update_manifest::{TrustedKeys, channel::Channel, select::Check};

const UNVERIFIED_INSTALLATION: &str = "Could not read the installed version. Automatic installation is unavailable; use the downloaded package to repair the installation.";

/// A manual check bypasses scheduling and cached manifests without enabling automatic checks.
#[derive(Clone, Copy, Eq, PartialEq)]
enum FetchMode {
    Scheduled,
    Fresh,
    Cached,
    Manual,
}

#[derive(Deserialize, Serialize)]
struct Stamp {
    checked_at: u64,
    current: String,
    #[serde(default)]
    build: u64,
}

/// The destination stays known even when automatic installation cannot verify its build.
pub(super) struct Installation {
    pub(super) target: PathBuf,
    pub(super) build: Option<update_manifest::build::Build>,
}

/// A signed manifest can outlive the retained download it names; fetch the channel again.
#[derive(Debug, thiserror::Error)]
#[error("the update download is no longer published")]
pub(super) struct MissingArtifact;

/// A missing served document or payload warrants one refresh of the signed channel manifest.
pub(super) fn check_missing(status: reqwest::StatusCode) -> Result<()> {
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(MissingArtifact.into());
    }
    Ok(())
}

/// Refreshes a missing payload's manifest once, leaving every other failure for the next check.
pub(super) fn retry_missing(
    mut check: impl FnMut() -> Result<()>,
    active: impl FnOnce() -> bool,
) -> Result<()> {
    let result = check();
    if result
        .as_ref()
        .is_err_and(|error| error.is::<MissingArtifact>())
        && active()
    {
        check()
    } else {
        result
    }
}

/// The signed channel manifest has not caught up with a release push yet.
#[derive(Debug, thiserror::Error)]
#[error("the signed manifest is older than the announced release")]
struct ManifestBehind;

/// Waits on the edge clock while allowing preference changes to cancel a delayed check promptly.
fn wait_announcement(generation: u64, mut hint: Announcement) -> Option<Announcement> {
    loop {
        let state = STATE.lock().ok()?;
        if !state.accepts_worker(generation) {
            return None;
        }
        hint = state.announcement.clone().unwrap_or(hint);
        let remaining = hint.not_before.saturating_sub(now_secs());
        drop(state);
        if remaining == 0 {
            return Some(hint);
        }
        std::thread::park_timeout(Duration::from_secs(remaining.min(1)));
    }
}

/// Runs one worker at a time and retries when a preference changes during its request.
pub(super) fn start(force: bool) {
    let Some(layout) = LAYOUT.get().cloned() else {
        return;
    };
    let (preferences, switch_now, mode, generation, announcement) = {
        let Ok(mut state) = STATE.lock() else {
            return;
        };
        if state.updater.is_none()
            || state.running
            || state.requested
            || state.startup != StartupPhase::Complete
        {
            return;
        }
        state.running = true;
        state.status = Status::Checking;
        let check_now = std::mem::take(&mut state.check_now);
        let manual = std::mem::take(&mut state.manual_check);
        let mode = if manual {
            FetchMode::Manual
        } else if force {
            FetchMode::Cached
        } else if check_now || state.announcement.is_some() {
            FetchMode::Fresh
        } else {
            FetchMode::Scheduled
        };
        (
            state.preferences,
            state.switch_now,
            mode,
            state.generation,
            (!force && !check_now)
                .then(|| state.announcement.clone())
                .flatten(),
        )
    };
    std::thread::spawn(move || {
        let announcement = announcement.and_then(|hint| wait_announcement(generation, hint));
        let result = retry_missing(
            || {
                run(
                    &layout,
                    preferences,
                    switch_now,
                    generation,
                    mode,
                    announcement.as_ref(),
                )
            },
            || active(generation),
        );
        if let Err(error) = &result {
            tracing::debug!("update check: {error:#}");
        }
        let (changed, pending) = if let Ok(mut state) = STATE.lock() {
            state.finish_announcement(
                announcement.as_ref(),
                result
                    .as_ref()
                    .is_err_and(|error| error.is::<ManifestBehind>()),
                now_secs(),
                announcements::jitter(),
            );
            let changed = state.finish_check(generation, switch_now, result.is_err());
            (changed, state.check_now || state.announcement.is_some())
        } else {
            (false, false)
        };
        if changed {
            start(true);
        } else if pending {
            // A notice that arrived during this check may name a release it started too early to see.
            start(false);
        }
    });
}

/// Rejects stale requests before publishing a stage or consuming another response chunk.
fn active(generation: u64) -> bool {
    STATE
        .lock()
        .is_ok_and(|state| state.accepts_worker(generation))
}

/// Reopens a verified ready stage, or checks the selected channel and resumes its artifact.
/// Manual checks bypass the completion stamp, cached manifests and the automatic-check preference.
fn run(
    layout: &InstallLayout,
    preferences: Preferences,
    switch_now: bool,
    generation: u64,
    mode: FetchMode,
    announcement: Option<&Announcement>,
) -> Result<()> {
    let Some(_) = preferences.channel.manifest_url() else {
        ensure!(mode != FetchMode::Manual, "unpublished update channel");
        return Ok(());
    };
    let keys = update_manifest::parse_keys(update_installer::TRUSTED_KEYS)?;
    if keys.is_empty() {
        ensure!(
            mode != FetchMode::Manual,
            "no trusted update keys configured"
        );
        return Ok(());
    }
    let platform = update_manifest::platform_key(std::env::consts::OS, std::env::consts::ARCH)
        .context("unsupported platform")?;
    let root = layout.update_dir();
    fs::create_dir_all(&root)?;
    update_installer::helper::prune(&root, SystemTime::now());
    // The updater's worker and offline helper share this transaction lease.
    let lease =
        update_installer::lease::Lease::acquire(&root).context("lock the update directory")?;
    let installation = installation(layout);
    let running_build = update_manifest::build::Build::current();
    let current = installation
        .as_ref()
        .and_then(|installation| installation.build.as_ref())
        .unwrap_or(&running_build);
    let verified = installation
        .as_ref()
        .is_none_or(|installation| installation.build.is_some());
    if !verified
        && let Ok(mut state) = STATE.lock()
        && state.accepts_worker(generation)
    {
        if let Some(ready) = state.ready.as_mut() {
            ready.notice.writable = false;
        }
        state.message = Some(UNVERIFIED_INSTALLATION.into());
    }
    #[cfg(target_os = "linux")]
    if let Some(running) = MANUAL.get()
        && STATE.lock().is_ok_and(|state| state.ready.is_none())
    {
        let ready = running.recover(
            &layout.user_data_root,
            &keys,
            preferences.channel,
            &platform,
            &lease,
            OffsetDateTime::now_utc(),
        );
        if let Some(ready) = ready {
            if let Ok(mut state) = STATE.lock() {
                state.offer(ready, generation);
                if state.accepts_worker(generation) {
                    state.status = Status::Idle;
                    state.message = Some("This downloaded update is running temporarily. Choose Restart and update to replace the installed copy.".into());
                }
            }
        }
    }
    let check = Check {
        channel: preferences.channel.name(),
        platform: &platform,
        current: &current.version,
        current_build: current.number,
        switch_now,
    };
    // Polling stamps belong to this process's build, even while it repairs an older installation.
    let stamp_check = Check {
        current: &running_build.version,
        current_build: running_build.number,
        ..check
    };
    let stable = Check {
        channel: Channel::Stable.name(),
        switch_now: true,
        ..check
    };
    let stable_cache = cache_path(&root, Channel::Stable);
    let stable_url = Channel::Stable.manifest_url();
    if stable_url.is_some()
        && read_cached(&stable_cache).is_some_and(|cached| {
            Selection::verify(&cached, &keys, stable, OffsetDateTime::now_utc()).is_ok()
        })
        && let Ok(mut state) = STATE.lock()
        && state.generation == generation
    {
        state.stable_available = true;
    }
    let destination = root.join("ready");
    stage::recover(&destination, &keys, check, OffsetDateTime::now_utc())?;
    // A postponed stage stays offered while periodic checks continue, and a newer release replaces it.
    let retained = STATE.lock().ok().and_then(|state| state.ready.clone());
    let mut staged = retained_manifest(retained.as_ref(), &keys, check, OffsetDateTime::now_utc());
    if retained.is_some()
        && staged.is_none()
        && let Ok(mut state) = STATE.lock()
        && state.accepts_worker(generation)
    {
        state.ready = None;
    }
    // A fresh signed envelope can still authenticate an unverifiable payload after expiry or a channel change.
    if destination.exists()
        && let Ok(selected) = stage::verify(&destination, &keys, check, OffsetDateTime::now_utc())
        && supersedes(&selected.manifest, staged.as_ref(), false)?
    {
        staged = Some(selected.manifest.clone());
        let (directory, reader) = stage::retain(&destination, &lease, &selected)?;
        publish(
            directory,
            reader,
            selected,
            check,
            installation
                .as_ref()
                .map(|installation| installation.target.clone()),
            verified,
            generation,
        );
    }
    if (!preferences.checks && !switch_now && mode != FetchMode::Manual) || !active(generation) {
        return Ok(());
    }
    let stamp_path = stamp_path(&root, preferences.channel);
    if mode == FetchMode::Scheduled
        && !switch_now
        && !due(last_check(&root, preferences.channel), now_secs())
    {
        // The skipped check would have found the same release, so restore what it offered.
        let switch_available =
            staged.is_none() && cached_switch_offer(&root, &keys, check, OffsetDateTime::now_utc());
        if let Ok(mut state) = STATE.lock()
            && state.generation == generation
        {
            set_switch_offer(&mut state, switch_available);
        }
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = http_client()?;
    runtime.block_on(async {
        if preferences.channel != Channel::Stable
            && let Some(url) = &stable_url
            && let Ok(bytes) = fetch(&client, url).await
            && Selection::verify(&bytes, &keys, stable, OffsetDateTime::now_utc()).is_ok()
        {
            let _ = fs::write(&stable_cache, &bytes);
            if let Ok(mut state) = STATE.lock()
                && state.generation == generation
            {
                state.stable_available = true;
            }
        }
        ensure!(active(generation), "update channel changed");
        let selected = releases::select(
            &root,
            &keys,
            check,
            mode == FetchMode::Cached,
            OffsetDateTime::now_utc(),
            async |channel| {
                fetch(
                    &client,
                    &channel
                        .manifest_url()
                        .context("unpublished update channel")?,
                )
                .await
            },
        )
        .await?;
        if announcement.is_some_and(|hint| {
            hint.ahead_of(&selected.manifest.version, selected.manifest.build_number)
        }) {
            return Err(ManifestBehind.into());
        }
        let switch_available = staged.is_none() && older_release(&selected.manifest, check)?;
        if let Ok(mut state) = STATE.lock()
            && state.generation == generation
        {
            set_switch_offer(&mut state, switch_available);
        }
        if !switch_now
            && !update_manifest::select::newer_build(
                &selected.manifest.version,
                selected.manifest.build_number,
                check.current,
                check.current_build,
            )?
        {
            record(&stamp_path, stamp_check, now_secs())?;
            set_status(generation, Status::UpToDate);
            return Ok(());
        }
        if !supersedes(&selected.manifest, staged.as_ref(), switch_now)? {
            record(&stamp_path, stamp_check, now_secs())?;
            set_status(generation, Status::UpToDate);
            return Ok(());
        }
        ensure!(active(generation), "update check cancelled");
        let artifact = selected.artifact.clone();
        let artifact_cache = cache_path(
            &root,
            Channel::parse(&selected.manifest.channel).context("unknown release channel")?,
        );
        let expires = selected.manifest.expires;
        let candidates = [
            Some(destination.as_path()),
            retained.as_ref().map(|ready| ready.stage.as_path()),
        ];
        let reused = reuse_stage(&lease, &selected, candidates.into_iter().flatten())?;
        let directory = if let Some(directory) = reused {
            directory
        } else {
            set_status(generation, Status::Downloading);
            let mut download = Download::begin(&root, selected)?;
            let offset = download.offset();
            // Retain only the current partial transfer. A channel change can restart another digest safely.
            for entry in fs::read_dir(&root)? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(digest) = name.strip_suffix(".part")
                    && digest.len() == 64
                    && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && !digest.eq_ignore_ascii_case(&artifact.sha256)
                    && entry.file_type()?.is_file()
                {
                    fs::remove_file(entry.path())?;
                }
            }
            if offset < artifact.size as u64 {
                let mut request = client.get(&artifact.url).timeout(DOWNLOAD_TIMEOUT);
                if offset != 0 {
                    request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
                }
                let mut response = request.send().await?;
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    // The bounded retry must fetch a fresh manifest rather than reuse this cache.
                    let _ = fs::remove_file(&artifact_cache);
                }
                check_missing(response.status())?;
                if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                    let expected =
                        format!("bytes {offset}-{}/{}", artifact.size - 1, artifact.size);
                    ensure!(
                        response
                            .headers()
                            .get(reqwest::header::CONTENT_RANGE)
                            .and_then(|value| value.to_str().ok())
                            == Some(expected.as_str()),
                        "invalid resumed range"
                    );
                } else {
                    ensure!(
                        response.status() == reqwest::StatusCode::OK,
                        "artifact download failed: {}",
                        response.status()
                    );
                    if offset != 0 {
                        download.restart()?;
                    }
                }
                if let Some(length) = response.content_length() {
                    ensure!(
                        length == artifact.size as u64 - download.offset(),
                        "artifact content length mismatch"
                    );
                }
                while let Some(chunk) = response.chunk().await? {
                    ensure!(active(generation), "update check cancelled");
                    download.write(&chunk)?;
                }
            }
            ensure!(active(generation), "update check cancelled");
            download.finish(&destination)?
        };
        ensure!(
            expires > OffsetDateTime::now_utc(),
            "release manifest expired during download"
        );
        let selected = stage::verify(&directory, &keys, check, OffsetDateTime::now_utc())?;
        let (directory, reader) = stage::retain(&directory, &lease, &selected)?;
        publish(
            directory,
            reader,
            selected,
            check,
            installation
                .as_ref()
                .map(|installation| installation.target.clone()),
            verified,
            generation,
        );
        record(&stamp_path, stamp_check, now_secs())
    })
}

/// Publishes worker progress only while its channel and consent are still current.
fn set_status(generation: u64, status: Status) {
    if let Ok(mut state) = STATE.lock()
        && state.accepts_worker(generation)
    {
        state.status = status;
    }
}

/// Revalidates the offered files themselves, independently of the replaceable shared cache.
pub(super) fn retained_manifest(
    ready: Option<&Ready>,
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
) -> Option<update_manifest::Manifest> {
    let ready = ready?;
    stage::verify(
        &ready.stage,
        keys,
        Check {
            switch_now: ready.switch_now,
            ..check
        },
        now,
    )
    .ok()
    .map(|selected| selected.manifest)
}

/// The sole persisted completion timestamp used by both polling and the worker.
pub(super) fn stamp_path(root: &Path, channel: Channel) -> PathBuf {
    root.join(format!("last-check-{}.json", channel.name()))
}

/// Reads this running build's last successful check without moving its next deadline.
pub(super) fn last_check(root: &Path, channel: Channel) -> Option<u64> {
    let stamp: Stamp = serde_json::from_slice(&read_cached(&stamp_path(root, channel))?).ok()?;
    (stamp.current == env!("CARGO_PKG_VERSION") && stamp.build == update_manifest::build_number())
        .then_some(stamp.checked_at)
}

/// Renews an unchanged verified payload locally, leaving downloads for payloads that are absent.
pub(super) fn reuse_stage<'a>(
    lease: &update_installer::lease::Lease,
    selected: &Selection,
    candidates: impl IntoIterator<Item = &'a Path>,
) -> Result<Option<PathBuf>> {
    for candidate in candidates {
        if stage::refresh_envelope(candidate, lease, selected)? {
            return Ok(Some(candidate.to_owned()));
        }
    }
    Ok(None)
}

/// True when the channel's release is strictly older than the running build.
fn older_release(manifest: &update_manifest::Manifest, check: Check<'_>) -> Result<bool> {
    Ok(update_manifest::select::newer_build(
        check.current,
        check.current_build,
        &manifest.version,
        manifest.build_number,
    )?)
}

/// Explicit manual switches select their release; automatic checks need a newer build or expiry.
pub(super) fn supersedes(
    fetched: &update_manifest::Manifest,
    staged: Option<&update_manifest::Manifest>,
    switch_now: bool,
) -> Result<bool> {
    if switch_now {
        return Ok(true);
    }
    let Some(staged) = staged else {
        return Ok(true);
    };
    Ok(update_manifest::select::newer_build(
        &fetched.version,
        fetched.build_number,
        &staged.version,
        staged.build_number,
    )? || (fetched.version == staged.version
        && fetched.build_number == staged.build_number
        && fetched.expires > staged.expires))
}

/// Whether the cached winner for the selected channel warrants the Switch now offer.
pub(super) fn cached_switch_offer(
    root: &Path,
    keys: &TrustedKeys,
    check: Check<'_>,
    now: OffsetDateTime,
) -> bool {
    releases::cached(root, keys, check, now)
        .is_some_and(|selected| older_release(&selected.manifest, check).unwrap_or(false))
}

/// Refreshes automatic switch offers without replacing an active manual switch's progress.
pub(super) fn set_switch_offer(state: &mut State, available: bool) {
    if state.switch_now {
        return;
    }
    if available {
        offer_switch(state);
    } else if state.switch_available {
        state.switch_available = false;
        state.message = None;
    }
}

/// Offers an explicit switch to the selected channel's older release.
pub(super) fn offer_switch(state: &mut State) {
    state.switch_available = true;
    state.message = Some(
        "This channel has an older version. Wait for its next release, or choose Switch now."
            .into(),
    );
}

/// Publishes a verified artifact with the platform's supported installation path.
fn publish(
    stage: PathBuf,
    reader: File,
    selected: Selection,
    check: Check<'_>,
    target: Option<PathBuf>,
    verified: bool,
    generation: u64,
) {
    let newer = update_manifest::select::newer_build(
        &selected.manifest.version,
        selected.manifest.build_number,
        check.current,
        check.current_build,
    )
    .unwrap_or(false);
    let writable = newer
        && verified
        && target
            .clone()
            .map_or_else(handoff::installed_target, Ok)
            .and_then(|target| update_installer::target::access(&target))
            .is_ok_and(update_installer::target::Access::permits_update);
    if let Ok(mut state) = STATE.lock() {
        let mut ready = Ready::new(stage, reader, selected, check.switch_now, writable);
        ready.target = target;
        state.offer(ready, generation);
        if !verified && state.accepts_worker(generation) {
            state.message = Some(UNVERIFIED_INSTALLATION.into());
        }
    }
}

/// Compares manual-copy checks with the actual installation while preserving portable behavior.
fn installation(layout: &InstallLayout) -> Option<Installation> {
    #[cfg(target_os = "linux")]
    {
        let running = MANUAL.get()?;
        Some(inspect_installation(
            running.destination(&layout.user_data_root),
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = layout;
        None
    }
}

/// Queries build identity without discarding the installation's destination on failure.
#[cfg(target_os = "linux")]
pub(super) fn inspect_installation(target: PathBuf) -> Installation {
    let build = update_installer::target::installed_build(&target).ok();
    Installation { target, build }
}

/// Bounds the signed document separately from the much larger release artifact.
pub(super) async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await?;
    check_missing(response.status())?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "manifest download failed: {}",
        response.status()
    );
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= update_manifest::MAX_ENVELOPE_BYTES,
            "manifest exceeds size limit"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Shares the HTTPS-only policy and idle deadlines between checks and range requests.
pub(super) fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(FETCH_TIMEOUT)
        .read_timeout(FETCH_TIMEOUT)
        .build()?)
}

/// Reads the edge clock for the check stamp.
pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Records successful channel checks; failed or interrupted downloads remain retryable.
pub(super) fn record(path: &std::path::Path, check: Check<'_>, now: u64) -> Result<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().context("missing check directory")?)?;
    file.write_all(&serde_json::to_vec(&Stamp {
        checked_at: now,
        current: check.current.into(),
        build: check.current_build,
    })?)?;
    file.persist(path)?;
    Ok(())
}
