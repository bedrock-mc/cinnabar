use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, PoisonError},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use launcher::local_worlds::world::{Setup, SetupState, UnavailableReason};
use url::Url;

use super::{
    Cancel, create_private_dir, lock,
    world::{WorldError, failed},
    write_private,
};

/// Probes how the dedicated server can run here.
pub(crate) type Detector = Arc<dyn Fn() -> RuntimeInfo + Send + Sync>;
/// Runs for each detection result that becomes current.
pub(crate) type DetectedHook = Arc<dyn Fn(&RuntimeInfo) + Send + Sync>;
/// Accepts the hosts downloads and their redirects may reach.
pub(crate) type HostCheck = Arc<dyn Fn(&Url) -> bool + Send + Sync>;

/// How the dedicated server runs on this machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeKind {
    /// The official build run directly (Windows and Linux x86-64).
    Native,
    /// The Linux build in a Docker-compatible container (macOS).
    Container,
    /// The Linux build translated by an APK-bundled runtime.
    Translated,
    None,
}

impl RuntimeKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Container => "container",
            Self::Translated => "translated",
            Self::None => "none",
        }
    }
}

/// A detected runtime and why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeInfo {
    pub kind: RuntimeKind,
    pub reason: String,
    /// Set when the server cannot run only for want of Docker.
    pub unavailable: Option<UnavailableReason>,
}

impl RuntimeInfo {
    /// An experimental runtime that translates the Linux server on this host.
    pub(crate) fn translated() -> Self {
        Self {
            kind: RuntimeKind::Translated,
            reason: "experimental translated Bedrock Dedicated Server".to_owned(),
            unavailable: None,
        }
    }

    pub(crate) fn native() -> Self {
        Self {
            kind: RuntimeKind::Native,
            reason: "native Bedrock Dedicated Server".to_owned(),
            unavailable: None,
        }
    }

    /// The optimistic container runtime assumed while Docker is probed.
    pub(crate) fn checking() -> Self {
        Self {
            kind: RuntimeKind::Container,
            reason: "checking Docker".to_owned(),
            unavailable: None,
        }
    }

    pub(crate) fn none(reason: &str, unavailable: Option<UnavailableReason>) -> Self {
        Self {
            kind: RuntimeKind::None,
            reason: reason.to_owned(),
            unavailable,
        }
    }
}

/// An operating system and architecture, spelled as the server download names them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Platform {
    pub os: &'static str,
    pub arch: &'static str,
}

impl Platform {
    pub(crate) fn host() -> Self {
        Self {
            os: match std::env::consts::OS {
                "macos" => "darwin",
                other => other,
            },
            arch: match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                other => other,
            },
        }
    }

    /// Whether Mojang ships a dedicated server for this platform.
    pub(crate) fn runs_bds(self) -> bool {
        matches!(self.os, "windows" | "linux") && self.arch == "amd64"
    }

    /// Whether this host can use the desktop container runtime for BDS.
    pub(crate) fn runs_containers(self) -> bool {
        !matches!(self.os, "android" | "ios")
    }
}

/// Downloads the official Bedrock Dedicated Server on first use, after explicit EULA acceptance.
/// Nothing is bundled; builds live in `root/<version>/` beside a provenance manifest.
pub(crate) struct Provisioner {
    pub root: PathBuf,
    /// The exact build, fetched from its versioned official URL; empty asks the download service
    /// for its current build, which must match `version_prefix`.
    pub version: String,
    /// Empty derives the release line from `version`.
    pub version_prefix: String,
    pub platform: Platform,
    pub links_url: String,
    pub allow_host: HostCheck,
    pub stall_timeout: Duration,
    hooks: Mutex<Hooks>,
    state: Mutex<Progress>,
    /// Signalled when the last detection in flight lands.
    settled: Condvar,
    /// Keeps detection results and their hooks in generation order.
    detect_order: Mutex<()>,
    ensure_lock: Mutex<()>,
}

#[derive(Default)]
struct Hooks {
    detect: Option<Detector>,
    on_detected: Option<DetectedHook>,
}

#[derive(Default)]
struct Progress {
    /// `None` derives the runtime from the platform.
    runtime: Option<RuntimeInfo>,
    probing: usize,
    /// Generation of the newest detection or `set_runtime`.
    started: u64,
    /// Generation of the runtime in effect; older results are dropped.
    applied: u64,
    /// The step in progress while a BDS world starts.
    op: Option<SetupState>,
    done: u64,
    total: u64,
    layers: (u32, u32),
    version: Option<String>,
    last_error: Option<String>,
}

impl Provisioner {
    pub(crate) fn new(root: PathBuf, version: String) -> Self {
        Self {
            root,
            version,
            version_prefix: String::new(),
            platform: Platform::host(),
            links_url: super::download::LINKS_API.to_owned(),
            allow_host: Arc::new(super::download::official_host),
            stall_timeout: super::download::DEFAULT_STALL_TIMEOUT,
            hooks: Mutex::default(),
            state: Mutex::default(),
            settled: Condvar::new(),
            detect_order: Mutex::new(()),
            ensure_lock: Mutex::new(()),
        }
    }

    /// Records how the server will run, superseding detections still in flight.
    pub(crate) fn set_runtime(&self, info: RuntimeInfo) {
        let mut state = lock(&self.state);
        state.started += 1;
        state.applied = state.started;
        state.runtime = Some(info);
    }

    /// Sets the probe [`Self::redetect`] runs.
    pub(crate) fn set_detector(&self, detect: Detector) {
        lock(&self.hooks).detect = Some(detect);
    }

    pub(crate) fn on_detected(&self, hook: DetectedHook) {
        lock(&self.hooks).on_detected = Some(hook);
    }

    /// Re-probes the runtime (after the user starts Docker) and returns the runtime in effect, which
    /// is a newer probe's result when one overtook this one.
    pub(crate) fn redetect(&self) -> RuntimeInfo {
        let (generation, detect) = self.begin_detect();
        self.finish_detect(generation, detect.map(|detect| detect()));
        self.runtime_info()
    }

    /// Assumes `assume` until the detector's result lands; status reports `checking_runtime` meanwhile.
    pub(crate) fn detect_in_background(self: &Arc<Self>, assume: RuntimeInfo) {
        self.set_runtime(assume);
        let (generation, detect) = self.begin_detect();
        let provisioner = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("bds-runtime-probe".to_owned())
            .spawn(move || {
                let info = detect.map(|detect| detect());
                provisioner.finish_detect(generation, info);
            });
        if let Err(error) = spawned {
            tracing::warn!("cannot probe for Docker: {error}");
            self.finish_detect(generation, None);
        }
    }

    /// Waits up to `timeout` for detections in flight; false when one is still running.
    pub(crate) fn await_runtime(&self, timeout: Duration) -> bool {
        let state = lock(&self.state);
        let (state, _) = self
            .settled
            .wait_timeout_while(state, timeout, |state| state.probing > 0)
            .unwrap_or_else(PoisonError::into_inner);
        state.probing == 0
    }

    fn begin_detect(&self) -> (u64, Option<Detector>) {
        let detect = lock(&self.hooks).detect.clone();
        let mut state = lock(&self.state);
        state.started += 1;
        state.probing += 1;
        (state.started, detect)
    }

    /// Applies `info` (`None` for no result) unless a newer generation already took effect.
    fn finish_detect(&self, generation: u64, info: Option<RuntimeInfo>) {
        let order = lock(&self.detect_order);
        let fresh = {
            let mut state = lock(&self.state);
            match info {
                Some(info) if generation > state.applied => {
                    state.applied = generation;
                    state.runtime = Some(info.clone());
                    Some(info)
                }
                _ => None,
            }
        };
        let hook = lock(&self.hooks).on_detected.clone();
        if let Some(info) = fresh
            && let Some(hook) = hook
        {
            hook(&info);
        }
        drop(order);
        let mut state = lock(&self.state);
        state.probing -= 1;
        if state.probing == 0 {
            self.settled.notify_all();
        }
    }

    pub(crate) fn runtime_info(&self) -> RuntimeInfo {
        if let Some(runtime) = lock(&self.state).runtime.clone() {
            return runtime;
        }
        if self.platform.runs_bds() {
            RuntimeInfo::native()
        } else {
            RuntimeInfo::none(
                "Bedrock Dedicated Server has no build for this platform",
                None,
            )
        }
    }

    /// The release line the client can join; empty when nothing is pinned.
    pub(super) fn prefix(&self) -> String {
        if !self.version_prefix.is_empty() {
            return self.version_prefix.clone();
        }
        let parts: Vec<&str> = self.version.split('.').collect();
        if parts.len() >= 3 {
            parts[..3].join(".")
        } else {
            String::new()
        }
    }

    pub(super) fn version_matches(&self, version: &str) -> bool {
        let prefix = self.prefix();
        !prefix.is_empty()
            && (version == prefix
                || version
                    .strip_prefix(&prefix)
                    .is_some_and(|rest| rest.starts_with('.')))
    }

    pub(super) fn binary_name(&self) -> &'static str {
        if self.download_platform().os == "windows" {
            "bedrock_server.exe"
        } else {
            "bedrock_server"
        }
    }

    /// The server archive's platform, independently of the host that runs it.
    pub(super) fn download_platform(&self) -> Platform {
        match self.runtime_info().kind {
            RuntimeKind::Container | RuntimeKind::Translated => Platform {
                os: "linux",
                arch: "amd64",
            },
            _ => self.platform,
        }
    }

    fn eula_path(&self) -> PathBuf {
        self.root.join("eula.json")
    }

    pub(crate) fn eula_accepted(&self) -> bool {
        fs::metadata(self.eula_path()).is_ok()
    }

    /// Records the user's acceptance of the Minecraft EULA and privacy policy.
    pub(crate) fn accept_eula(&self) -> Result<(), WorldError> {
        create_private_dir(&self.root)
            .map_err(|error| failed(format!("create server directory: {error}")))?;
        let accepted = serde_json::json!({
            "accepted_unix": unix_now(),
            "terms": "https://minecraft.net/eula",
        });
        write_private(&self.eula_path(), accepted.to_string().as_bytes())
            .map_err(|error| failed(format!("record EULA acceptance: {error}")))
    }

    /// The installed build matching the target version, as its binary and version.
    pub(crate) fn installed(&self) -> Option<(PathBuf, String)> {
        let entries = fs::read_dir(&self.root).ok()?;
        for entry in entries.flatten() {
            let Ok(version) = entry.file_name().into_string() else {
                continue;
            };
            if !entry.file_type().is_ok_and(|kind| kind.is_dir())
                || !self.version_matches(&version)
                || (!self.version.is_empty() && version != self.version)
            {
                continue;
            }
            let dir = entry.path();
            if fs::metadata(dir.join(super::unpack::INSTALL_MANIFEST)).is_err() {
                continue;
            }
            let binary = dir.join(self.binary_name());
            if fs::metadata(&binary).is_ok_and(|metadata| !metadata.is_dir()) {
                return Some((binary, version));
            }
        }
        None
    }

    /// The installation state and why the server cannot run, when only Docker is missing.
    pub(crate) fn status(&self) -> (Setup, Option<UnavailableReason>) {
        let runtime = self.runtime_info();
        let accepted = self.eula_accepted();
        let (op, done, total, layers, version, last_error, probing) = {
            let state = lock(&self.state);
            (
                state.op,
                state.done,
                state.total,
                state.layers,
                state.version.clone(),
                state.last_error.clone(),
                state.probing > 0,
            )
        };
        let mut setup = Setup {
            state: SetupState::NotInstalled,
            version,
            bytes_done: done,
            bytes_total: total,
            layers_done: layers.0,
            layers_total: layers.1,
            eula_accepted: accepted,
            error: None,
            runtime: runtime.kind.as_str().to_owned(),
            reason: Some(runtime.reason).filter(|reason| !reason.is_empty()),
        };
        if runtime.kind == RuntimeKind::None {
            setup.state = SetupState::Unsupported;
        } else if let Some(op) = op {
            setup.state = op;
        } else if probing {
            setup.state = SetupState::CheckingRuntime;
        } else if let Some((_, version)) = self.installed() {
            setup.state = SetupState::Ready;
            setup.version = Some(version);
        } else if !accepted {
            setup.state = SetupState::EulaRequired;
        } else if let Some(error) = last_error {
            setup.state = SetupState::Failed;
            setup.error = Some(error);
        }
        (setup, runtime.unavailable)
    }

    /// Reports the step in progress; `None` ends it.
    pub(crate) fn set_op(
        &self,
        op: Option<SetupState>,
        version: Option<&str>,
        done: u64,
        total: u64,
    ) {
        let mut state = lock(&self.state);
        state.op = op;
        state.version = version.map(str::to_owned);
        state.done = done;
        state.total = total;
        state.layers = (0, 0);
    }

    pub(crate) fn set_layers(&self, done: u32, total: u32) {
        lock(&self.state).layers = (done, total);
    }

    /// Ends the current step with `message`, the path-free text the client shows.
    pub(crate) fn fail_with(&self, message: &str, error: WorldError) -> WorldError {
        tracing::error!("dedicated server setup failed: {error}");
        let mut state = lock(&self.state);
        state.op = None;
        state.last_error = Some(message.to_owned());
        error
    }

    fn fail(&self, error: WorldError) -> WorldError {
        self.fail_with("dedicated server download failed", error)
    }

    /// The server binary, downloading and unpacking it first if needed. Container and translated
    /// runtimes acquire the Linux build independently of the host platform.
    pub(crate) fn ensure(&self, cancel: &Cancel) -> Result<PathBuf, WorldError> {
        let _ensuring = lock(&self.ensure_lock);
        if !matches!(
            self.runtime_info().kind,
            RuntimeKind::Native | RuntimeKind::Container | RuntimeKind::Translated
        ) {
            return Err(WorldError::BackendUnavailable);
        }
        if !self.eula_accepted() {
            return Err(WorldError::EulaRequired);
        }
        if let Some((binary, _)) = self.installed() {
            return Ok(binary);
        }
        lock(&self.state).last_error = None;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| self.fail(failed(error.to_string())))?;
        let client = self.client().map_err(|error| self.fail(error))?;
        let (version, link) = runtime
            .block_on(self.resolve(&client, cancel))
            .map_err(|error| self.fail(error))?;
        let (archive, digest, size) = runtime
            .block_on(self.download(&client, &version, &link, cancel))
            .map_err(|error| self.fail(error))?;
        // A failed download keeps its part to resume; a downloaded archive is always removed.
        let _archive = RemoveOnDrop(archive.clone());
        self.set_op(Some(SetupState::Unpacking), Some(&version), size, size);
        let binary = self
            .unpack(&archive, &version, &link, &digest, size)
            .map_err(|error| self.fail(error))?;
        self.prune_old_installs(&version);
        self.set_op(None, Some(&version), size, size);
        Ok(binary)
    }
}

/// Deletes a file when dropped.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// The install folder of an installed server binary.
pub(crate) fn install_dir(binary: &Path) -> Result<&Path, WorldError> {
    binary
        .parent()
        .ok_or_else(|| failed("the server binary has no install folder"))
}

#[cfg(test)]
#[path = "provision_tests.rs"]
mod tests;
