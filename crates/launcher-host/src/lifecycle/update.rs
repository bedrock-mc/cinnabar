//! One background signed-release check and the launcher's explicit update decisions.

mod announcements;
mod handoff;
#[cfg(target_os = "linux")]
mod manual;
mod releases;
#[cfg(test)]
mod tests;
mod worker;

use announcements::Announcement;

use launcher::{
    install_layout::InstallLayout,
    menu::{
        settings_options::{SETTINGS_FILE, SettingsOptions},
        updates::{Action, Preferences, Ready as Notice, Status, View},
    },
};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use time::OffsetDateTime;
use update_installer::startup;
use update_manifest::channel::Channel;

/// Fallback poll between launch checks and release notices.
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const CHECK_POLL_INTERVAL: Duration = Duration::from_secs(60);
const MANUAL_POLL_INTERVAL: Duration = Duration::from_secs(1);
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
static LAYOUT: OnceLock<InstallLayout> = OnceLock::new();
static STATE: Mutex<State> = Mutex::new(State::new());
#[cfg(target_os = "linux")]
static MANUAL: OnceLock<manual::Running> = OnceLock::new();
#[cfg(unix)]
static STARTUP: OnceLock<update_installer::recovery::Startup> = OnceLock::new();

#[derive(Clone)]
struct Ready {
    expires: OffsetDateTime,
    stage: PathBuf,
    notice: Notice,
    channel: Channel,
    switch_now: bool,
    /// The installed destination when this process runs from a downloaded manual package.
    target: Option<PathBuf>,
    /// Keeps the offered stage alive while a later download replaces the cache.
    lease: Option<std::sync::Arc<std::fs::File>>,
}

impl Ready {
    /// Presents the verified selection's version and expiry while retaining its own stage reader.
    fn new(
        stage: PathBuf,
        reader: std::fs::File,
        selected: update_installer::stage::Selection,
        switch_now: bool,
        writable: bool,
    ) -> Self {
        Self {
            expires: selected.manifest.expires,
            stage,
            channel: Channel::parse(&selected.manifest.channel).expect("verified release channel"),
            switch_now,
            target: None,
            lease: Some(std::sync::Arc::new(reader)),
            notice: Notice {
                version: selected.manifest.version,
                writable,
                protocol_required: false,
            },
        }
    }
}

struct State {
    manual_download: bool,
    updater: Option<update_installer::lease::Updater>,
    last_poll: Option<u64>,
    preferences: Preferences,
    ready: Option<Ready>,
    dismissed: bool,
    requested: bool,
    switch_now: bool,
    switch_available: bool,
    stable_available: bool,
    protocol_required: bool,
    message: Option<String>,
    generation: u64,
    running: bool,
    /// A launch or release notice wants a fresh check regardless of the completion stamp.
    check_now: bool,
    manual_check: bool,
    status: Status,
    announcement: Option<Announcement>,
    startup: StartupPhase,
}

/// Good frames authorize confirmation; update checks begin once recovery has finished.
#[derive(Eq, PartialEq)]
enum StartupPhase {
    Preparing,
    Confirming,
    Complete,
}

impl State {
    /// Polls the persisted completion stamp; skipped polls never move the next deadline.
    fn tick(&mut self, root: &std::path::Path, now: u64) -> bool {
        if self.running
            || self.requested
            || self.last_poll.is_some_and(|last| {
                now >= last
                    && now - last
                        < if self.manual_download && self.updater.is_none() {
                            MANUAL_POLL_INTERVAL
                        } else {
                            CHECK_POLL_INTERVAL
                        }
                        .as_secs()
            })
        {
            return false;
        }
        self.last_poll = Some(now);
        if self.updater.is_none() {
            self.updater = std::fs::create_dir_all(root)
                .and_then(|()| update_installer::lease::Updater::try_acquire(root))
                .ok();
            if self.updater.is_some() {
                // Restore downloaded offers even when the next network check is not due.
                return true;
            }
        }
        self.updater.is_some()
            && self.preferences.checks
            && due(worker::last_check(root, self.preferences.channel), now)
    }

    /// Clears old channel consent and requests an immediate check when the selection changes.
    fn configure(&mut self, preferences: Preferences) -> Option<bool> {
        if self.requested || self.preferences == preferences {
            return None;
        }
        let channel_changed = self.preferences.channel != preferences.channel;
        if channel_changed {
            self.announcement = None;
            self.ready = None;
            self.dismissed = false;
            self.switch_now = false;
            self.switch_available = false;
            self.message = None;
        }
        if !preferences.checks {
            self.announcement = None;
        }
        self.status = if self.running {
            Status::Checking
        } else {
            Status::Idle
        };
        self.manual_check = false;
        self.preferences = preferences;
        self.generation += 1;
        Some(channel_changed)
    }

    /// Requests a fresh network check without changing the automatic-check preference.
    fn request_check(&mut self) -> bool {
        if self.running || self.requested || self.startup != StartupPhase::Complete {
            return false;
        }
        self.manual_check = true;
        self.check_now = true;
        true
    }

    /// Requests one manual channel switch and replaces the switch offer with download progress.
    fn request_switch(&mut self) -> bool {
        if !self.switch_available || self.requested {
            return false;
        }
        self.switch_now = true;
        self.switch_available = false;
        self.message = Some("Downloading this channel's update…".into());
        self.dismissed = false;
        self.generation += 1;
        true
    }

    /// Ends a worker's progress on every outcome without clearing a newer action's message.
    fn finish_check(&mut self, generation: u64, switch_now: bool, failed: bool) -> bool {
        self.running = false;
        if self.generation != generation {
            return true;
        }
        self.status = if failed {
            Status::Failed
        } else if self.status.is_busy() {
            Status::Idle
        } else {
            self.status
        };
        if switch_now {
            self.switch_now = false;
            self.switch_available = failed;
            self.message =
                failed.then(|| "Could not download this channel's update. Try again.".into());
        }
        false
    }

    /// Replaces an offer only for the active worker and clears its completed progress message.
    fn offer(&mut self, ready: Ready, generation: u64) {
        if self.accepts_worker(generation) {
            if ready.channel == Channel::Stable {
                self.stable_available = true;
            }
            self.ready = Some(ready);
            self.switch_available = false;
            self.message = None;
        }
    }

    /// Holds one session's preferences and consent, without a persisted transaction state.
    const fn new() -> Self {
        Self {
            manual_download: false,
            updater: None,
            last_poll: None,
            preferences: Preferences::DEFAULT,
            ready: None,
            dismissed: false,
            requested: false,
            switch_now: false,
            switch_available: false,
            stable_available: false,
            protocol_required: false,
            message: None,
            generation: 0,
            running: false,
            check_now: false,
            manual_check: false,
            status: Status::Idle,
            announcement: None,
            startup: StartupPhase::Preparing,
        }
    }

    /// An explicit Later choice suppresses the optional quit-time install for this session too.
    fn applies_on_exit(&self, now: OffsetDateTime) -> bool {
        self.ready.as_ref().is_some_and(|ready| {
            ready.notice.writable
                && ready.expires > now
                && (self.requested
                    || (ready.target.is_none()
                        && self.preferences.install_on_quit
                        && !self.dismissed))
        })
    }

    /// Freezes the offered stage and cancels background work before accepting shutdown.
    fn request_restart(&mut self, now: OffsetDateTime) -> bool {
        if !self
            .ready
            .as_ref()
            .is_some_and(|ready| ready.notice.writable && ready.expires > now)
        {
            return false;
        }
        self.requested = true;
        self.generation += 1;
        true
    }

    /// Coalesces newer release hints on the selected channel without trusting their payloads.
    fn release_published(
        &mut self,
        release: &bridge::CinnabarRelease,
        current: (&str, u64),
        now: u64,
        jitter: u64,
    ) -> bool {
        if !self.preferences.checks || release.channel != self.preferences.channel.name() {
            return false;
        }
        let hint = Announcement::new(&release.version, release.build, now, jitter);
        // Older v1 notices omit the build; their same-version pushes still request a fresh check.
        let legacy = release.build == 0 && release.version == current.0;
        if (!legacy && !hint.ahead_of(current.0, current.1))
            || self
                .announcement
                .as_ref()
                .is_some_and(|pending| !hint.ahead_of(&pending.version, pending.build))
        {
            return false;
        }
        self.announcement = Some(hint);
        true
    }

    /// Retains a bounded retry only for the hint this worker actually checked.
    fn finish_announcement(
        &mut self,
        checked: Option<&Announcement>,
        behind: bool,
        now: u64,
        jitter: u64,
    ) {
        if checked.is_some()
            && self.announcement.as_ref() == checked
            && (!behind
                || !self
                    .announcement
                    .as_mut()
                    .is_some_and(|hint| hint.retry(now, jitter)))
        {
            self.announcement = None;
        }
    }

    /// Only the current worker may change stages, and accepted shutdown freezes its choice.
    fn accepts_worker(&self, generation: u64) -> bool {
        self.generation == generation && !self.requested
    }

    /// Hands an accepted Restart to the helper even if expiry changes during shutdown.
    fn handoff_after_exit(&mut self, now: OffsetDateTime) -> Option<Ready> {
        if self.requested {
            self.ready
                .as_ref()
                .filter(|ready| ready.notice.writable)
                .cloned()
        } else if self.applies_on_exit(now) && self.request_restart(now) {
            self.ready.clone()
        } else {
            None
        }
    }
}

/// Configures installed clients; stage and network checks begin after startup acknowledgement.
pub fn check_in_background(layout: &InstallLayout) {
    if !layout.is_installed() {
        return;
    }
    let _ = LAYOUT.set(layout.clone());
    #[cfg(target_os = "linux")]
    if let Some(running) = manual::Running::detect(layout) {
        let _ = MANUAL.set(running);
        if let Ok(mut state) = STATE.lock() {
            state.manual_download = true;
        }
    }
    tick(OffsetDateTime::now_utc());
    #[cfg(unix)]
    if let Ok(startup) =
        handoff::installed_target().and_then(update_installer::recovery::Startup::observe)
    {
        let _ = STARTUP.set(startup);
    }
    configure(
        SettingsOptions::load(&layout.user_config_root.join(SETTINGS_FILE)).update_preferences(),
    );
    worker::start(false);
}

/// Shares live preferences with the worker; channel changes always clear earlier consent.
pub fn configure(mut preferences: Preferences) {
    if std::env::var("CINNABAR_UPDATE_CHECK").is_ok_and(|value| value == "0") {
        preferences.checks = false;
    }
    let force = STATE
        .lock()
        .ok()
        .and_then(|mut state| state.configure(preferences));
    if let Some(force) = force {
        worker::start(force);
    }
}

/// Presents a verified ready stage and channel availability through the launcher data contract.
pub fn view() -> View {
    let now = OffsetDateTime::now_utc();
    tick(now);
    let Ok(state) = STATE.lock() else {
        return View {
            active: false,
            ..View::default()
        };
    };
    if state.updater.is_none() {
        return View {
            active: false,
            manual_pending: state.manual_download,
            ..View::default()
        };
    }
    View {
        ready: state
            .ready
            .as_ref()
            .filter(|ready| !state.dismissed && ready.expires > now)
            .map(|ready| {
                let mut notice = ready.notice.clone();
                notice.protocol_required = state.protocol_required;
                notice
            }),
        status: state.status,
        stable_available: state.stable_available,
        switch_available: state.switch_available,
        message: state.message.clone(),
        ..View::default()
    }
}

/// Raises the wording after a protocol refusal; it never changes update consent.
pub fn protocol_required() {
    if let Ok(mut state) = STATE.lock() {
        state.protocol_required = true;
    }
}

/// Returns true only when a verified, writable stage should trigger normal client shutdown.
pub fn act(action: Action) -> bool {
    let now = OffsetDateTime::now_utc();
    let mut download = None;
    let mut switch = false;
    let mut check = false;
    let quit = if let Ok(mut state) = STATE.lock() {
        if state.updater.is_none() {
            return false;
        }
        if state.requested {
            return true;
        }
        match action {
            Action::CheckNow => {
                check = state.request_check();
                false
            }
            Action::Restart => state.request_restart(now),
            Action::Later => {
                state.dismissed = true;
                state.requested = false;
                false
            }
            Action::Download => {
                download = state.ready.clone();
                false
            }
            Action::SwitchNow => {
                switch = state.request_switch();
                false
            }
        }
    } else {
        false
    };
    if let Some(ready) = download {
        handoff::open_download(ready);
    }
    if switch {
        worker::start(true);
    } else if check {
        worker::start(false);
    }
    quit
}

/// Starts the offline helper only for a click or the separately saved quit-time opt-in.
pub fn apply_after_exit() {
    let ready = STATE
        .lock()
        .ok()
        .filter(|state| state.updater.is_some())
        .and_then(|mut state| state.handoff_after_exit(OffsetDateTime::now_utc()));
    if let (Some(layout), Some(ready)) = (LAYOUT.get(), ready)
        && let Err(error) = handoff::spawn_or_restart(
            || handoff::spawn(layout, &ready),
            || {
                handoff::apply_target(&ready).and_then(|target| {
                    startup::Tree::spawn(&mut startup::client_command(&target))
                        .map(drop)
                        .map_err(Into::into)
                })
            },
        )
    {
        tracing::warn!("could not start updater: {error:#}");
    }
}

/// Signals a complete first frame to the waiting helper.
pub fn acknowledge_startup() {
    let Some(layout) = LAYOUT.get() else {
        return;
    };
    let marker = startup_marker(layout);
    if let Ok(mut state) = STATE.lock() {
        state.startup = StartupPhase::Confirming;
    }
    #[cfg(unix)]
    let update_dir = layout.update_dir();
    std::thread::spawn(move || {
        if let Some(marker) = marker {
            let _ = std::fs::File::options()
                .write(true)
                .create_new(true)
                .open(marker);
        }
        #[cfg(unix)]
        if let Some(startup) = STARTUP.get()
            && let Err(error) = startup.confirm(&update_dir)
        {
            tracing::debug!("update recovery: {error:#}");
        }
        if let Ok(mut state) = STATE.lock() {
            state.startup = StartupPhase::Complete;
            state.check_now = true;
        }
        worker::start(false);
    });
}

/// Lets a copied helper restore an orphaned replacement only after this failed client has exited.
pub fn recover_after_failed_start() {
    #[cfg(unix)]
    if STATE
        .lock()
        .is_ok_and(|state| state.updater.is_some() && state.startup == StartupPhase::Preparing)
        && let Some(layout) = LAYOUT.get()
        && let Ok(target) = handoff::installed_target()
        && update_installer::recovery::needs_rollback(&target, &layout.update_dir())
        && let Err(error) = handoff::spawn_recovery(layout, target)
    {
        tracing::warn!("could not start update recovery: {error:#}");
    }
}

/// Runs `prepare`, relaying each change to its `progress` report to a waiting update helper;
/// `awaiting_user` marks reports during which the helper must not time out.
pub fn while_preparing<T>(
    layout: &InstallLayout,
    progress: &std::path::Path,
    awaiting_user: fn(&[u8]) -> bool,
    prepare: impl FnOnce() -> T,
) -> T {
    let Some(marker) = startup_marker(layout) else {
        return prepare();
    };
    let mut relay = startup::Relay::start(marker, progress.to_owned(), awaiting_user);
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let watcher = std::thread::spawn(move || {
        while stopped.recv_timeout(startup::PROGRESS_POLL)
            == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        {
            relay.poll();
        }
        relay
    });
    let result = prepare();
    drop(stop);
    if let Ok(relay) = watcher.join() {
        relay.finish();
    }
    result
}

/// Whether this process, or the client that started it, was launched by the update helper.
pub fn launched_by_updater() -> bool {
    std::env::var_os(startup::STARTUP_ENV).is_some()
}

/// The helper's marker, accepted only inside this install's update directory.
fn startup_marker(layout: &InstallLayout) -> Option<PathBuf> {
    let marker = PathBuf::from(std::env::var_os(startup::STARTUP_ENV)?);
    (marker.starts_with(layout.update_dir())
        && marker
            .file_name()
            .is_some_and(|name| name == startup::STARTED))
    .then_some(marker)
}

/// Starts a fresh signed-manifest check for a release the Cinnabar API announced.
pub fn release_published(release: &bridge::CinnabarRelease) {
    let wanted = STATE.lock().is_ok_and(|mut state| {
        state.release_published(
            release,
            (env!("CARGO_PKG_VERSION"), update_manifest::build_number()),
            worker::now_secs(),
            announcements::jitter(),
        )
    });
    if wanted {
        worker::start(false);
    }
}

/// A corrected clock cannot suppress the next periodic check.
fn due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|last| now < last || now - last >= CHECK_INTERVAL.as_secs())
}

/// Retries the lifetime updater role and starts a check only when this process's poll is due.
fn tick(now: OffsetDateTime) {
    let Some(layout) = LAYOUT.get() else {
        return;
    };
    let now = now.unix_timestamp().max(0) as u64;
    let run = if let Ok(mut state) = STATE.lock() {
        state.tick(&layout.update_dir(), now)
    } else {
        false
    };
    if run {
        worker::start(false);
    }
}
