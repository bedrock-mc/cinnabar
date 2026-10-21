//! Per-user mod lists; admission, reload and settings retirement stay off the frame thread.

mod candidate;
mod install;
pub(super) mod presentation;
pub(super) mod removal;
mod schema;
pub(super) mod selection;
mod source;
mod status;
#[cfg(test)]
mod tests;
mod worker;

use super::admission::SettingsSeed;
use super::{ModRuntime, RELOAD_INTERVAL, VisualTimeOverride, interaction::ModInteraction};
use assets::RuntimeFontCatalog;
use bevy::prelude::*;
use candidate::{Candidate, Runtime as CandidateRuntime};
use client_shell::ui_resources::UiPresentationResource;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use mod_host::{ModHost, SettingsSnapshot};
pub(super) use schema::Entry;
use schema::{Registration, VERSION};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

pub(super) const REGISTRATION_FILE: &str = "local-mod.json";
pub(super) const STATUS_FILE: &str = "local-mod.status.json";
const MAX_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BYTES: usize = 64;

/// Runtime metadata belongs to one instance and moves with it during reordering.
pub(super) struct HostMetadata {
    pub(super) font: Option<Arc<RuntimeFontCatalog>>,
    pub(super) branding:
        Option<Arc<client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding>>,
    pub(super) owner: mod_ui::PanelUiOwner,
    pub(super) registration: Option<RegisteredHost>,
}
impl HostMetadata {
    /// Creates metadata for a stable environment-loaded panel owner.
    pub(super) fn environment(index: usize) -> Self {
        Self {
            font: None,
            branding: None,
            owner: mod_ui::PanelUiOwner::Environment(index),
            registration: None,
        }
    }
}

#[derive(Clone)]
pub(super) struct RegisteredHost {
    pub(super) entry: Entry,
    pub(super) identity: [u8; 32],
}

struct SourceSnapshot {
    registration: Entry,
    identity: [u8; 32],
    component: Vec<u8>,
    drawing: Option<super::admission::DrawingContext>,
    font: Option<Vec<u8>>,
    ui: Option<Vec<u8>>,
    settings: SettingsSeed,
}

impl SourceSnapshot {
    /// Retains the live-world fence captured for this snapshot.
    fn drawing_policy(&self) -> Option<mod_api::drawing::DrawingSession> {
        self.drawing.as_ref().map(|drawing| drawing.policy())
    }

    /// Selects source edits and recoverable fence changes without reviving a stopped identity.
    fn needs_admission(&self, admitted: Option<&Admitted>) -> bool {
        let Some(admitted) = admitted else {
            return true;
        };
        if admitted.source != self.identity {
            return true;
        }
        match &self.drawing {
            Some(drawing) if !drawing.restartable() => false,
            Some(drawing) if drawing.needs_restart() => true,
            _ => admitted.drawing != self.drawing_policy(),
        }
    }

    /// Prefers committed preferences to captured disk input, within the requested consent.
    fn settings_seed(&self, current: Option<&SettingsSnapshot>) -> SettingsSeed {
        match current {
            Some(current) => super::admission::settings_seed(
                self.registration.grants.settings,
                Some(current),
                &self.registration.component,
            ),
            None => self.settings.clone(),
        }
    }
}
struct Change {
    entry: Entry,
    result: Result<Box<Candidate>, String>,
}
struct Update {
    generation: u64,
    registration: Registration,
    changes: Vec<Change>,
}
struct LoadRequest {
    generation: u64,
    registration: Registration,
    sources: Vec<(Entry, Result<SourceSnapshot, String>)>,
}

/// Last accepted or refused reload, reported independently for each card.
#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct Status {
    component: PathBuf,
    state: String,
    /// The reload counter actually admitted or rejected, independent of list request IDs.
    #[serde(default)]
    reload: Option<u64>,
    #[serde(default)]
    message: Option<String>,
}
impl Status {
    /// Bounds guest and file error text before exposing it to the Settings card.
    fn new(component: PathBuf, state: &str, message: Option<String>, reload: Option<u64>) -> Self {
        Self {
            component,
            state: state.into(),
            reload,
            message: message.map(|message| status::bounded_message(&message)),
        }
    }
}
#[derive(serde::Deserialize, serde::Serialize)]
struct StatusList {
    version: u32,
    independent_registration: bool,
    client_pid: u32,
    request_id: String,
    mods: Vec<Status>,
}
impl StatusList {
    /// Identifies this client's independently managed registration status format.
    fn new(request_id: String, mods: Vec<Status>) -> Self {
        Self {
            version: VERSION,
            independent_registration: true,
            client_pid: std::process::id(),
            request_id,
            mods,
        }
    }
}

#[derive(Default)]
struct Desired {
    generation: u64,
    registration: Option<Registration>,
}
/// Source and fence accepted by the frame for one registration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Admitted {
    source: [u8; 32],
    drawing: Option<mod_api::drawing::DrawingSession>,
}

/// Off-frame disposal owns both frame guests and compiled packet programs.
pub(super) enum Retired {
    Frame(Box<ModHost>),
    Packet(Arc<mod_host::packet_mod::CompiledPacketMod>),
}

impl Retired {
    /// Releases executable resources on the loader worker.
    fn dispose(self) {
        match self {
            Self::Frame(host) => drop(host),
            Self::Packet(program) => drop(program),
        }
    }
}

impl From<ModHost> for Retired {
    fn from(host: ModHost) -> Self {
        Self::Frame(Box::new(host))
    }
}

impl Candidate {
    /// Returns the settings-owning frame guest when this is a frame candidate.
    fn frame_host(&self) -> Option<&ModHost> {
        match &self.runtime {
            CandidateRuntime::Frame(prepared) => Some(&prepared.host),
            CandidateRuntime::Packet(_) => None,
        }
    }

    /// Returns the mutable frame guest for frame-only admission fixtures.
    #[cfg(test)]
    fn frame_host_mut(&mut self) -> Option<&mut ModHost> {
        match &mut self.runtime {
            CandidateRuntime::Frame(prepared) => Some(&mut prepared.host),
            CandidateRuntime::Packet(_) => None,
        }
    }

    /// Moves executable resources into the registration worker's retirement lane.
    fn retire(self, watcher: &Watcher) {
        match self.runtime {
            CandidateRuntime::Frame(prepared) => watcher.retire(prepared.host),
            CandidateRuntime::Packet(program) => watcher.retire(Retired::Packet(program)),
        }
    }
}

enum Message {
    Acknowledge {
        generation: u64,
        request_id: String,
        installed: BTreeMap<PathBuf, Admitted>,
        statuses: Vec<Status>,
    },
    AwaitRetirement {
        selection: selection::Receipt,
        complete: removal::Completion,
    },
    Failed {
        component: PathBuf,
        reload: u64,
        error: String,
    },
    Retry {
        generation: u64,
        inactive: Vec<PathBuf>,
    },
    Stop,
}

#[derive(Resource)]
pub(crate) struct Watcher {
    updates: Receiver<Update>,
    messages: Sender<Message>,
    retired: Sender<Retired>,
    latest: Arc<AtomicU64>,
    applied: AtomicU64,
    desired: Arc<Mutex<Desired>>,
    alive: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pending: Mutex<Option<Update>>,
    acknowledgments: Mutex<Vec<Message>>,
    fallback_retired: Mutex<Vec<Retired>>,
    settings: Arc<Mutex<BTreeMap<PathBuf, SettingsSnapshot>>>,
    companions: Arc<Mutex<Vec<PathBuf>>>,
    drawing: Arc<Mutex<BTreeMap<PathBuf, super::admission::DrawingContext>>>,
}
impl Watcher {
    /// Keeps new sessions pending until the observed mod selection has finished admission.
    pub(crate) fn preparing(&self) -> bool {
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }
        let desired = self.desired.lock().expect("mod desired list");
        desired.registration.is_none() || self.applied.load(Ordering::Acquire) != desired.generation
    }
    /// Gives the library a fence that waits off-frame for final settings writes.
    pub(super) fn removal_client(&self) -> removal::Client {
        removal::Client::new(
            self.messages.clone(),
            self.latest.clone(),
            self.companions.clone(),
        )
    }
    /// Starts bounded list observation and a separate admission/retirement worker.
    pub(super) fn start(directory: PathBuf) -> Result<Self, String> {
        let (send_updates, updates) = bounded(1);
        let (messages, receive_messages) = bounded(8);
        let (retired, receive_retired) = bounded(mod_api::MAX_LOADED_MODS * 2);
        let latest = Arc::new(AtomicU64::new(0));
        let desired = Arc::new(Mutex::new(Desired::default()));
        let alive = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(BTreeMap::new()));
        let drawing = Arc::new(Mutex::new(BTreeMap::new()));
        let fences = drawing.clone();
        let (generation, selection, life, current) = (
            latest.clone(),
            desired.clone(),
            alive.clone(),
            settings.clone(),
        );
        let thread = thread::Builder::new()
            .name("local-mod-registration".into())
            .spawn(move || {
                worker::run(
                    directory,
                    receive_messages,
                    send_updates,
                    receive_retired,
                    generation,
                    selection,
                    current,
                    fences,
                );
                life.store(false, Ordering::Release);
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            updates,
            messages,
            retired,
            latest,
            applied: AtomicU64::new(0),
            desired,
            alive,
            thread: Some(thread),
            pending: Mutex::new(None),
            acknowledgments: Mutex::new(Vec::new()),
            fallback_retired: Mutex::new(Vec::new()),
            companions: Arc::new(Mutex::new(Vec::new())),
            settings,
            drawing,
        })
    }
    /// Publishes every surviving settings writer, including environment-loaded instances.
    fn remember_companions(&self, runtime: Option<&ModRuntime>) {
        *self.companions.lock().expect("mod settings owners") = runtime
            .into_iter()
            .flat_map(|runtime| {
                (0..runtime.hosts.host_count()).filter_map(|index| {
                    runtime.hosts.host(index).settings_path().map(PathBuf::from)
                })
            })
            .collect();
    }
    /// Captures each mod's latest in-memory preferences before preparing replacements.
    pub(super) fn remember_settings(&self, runtime: Option<&ModRuntime>) {
        self.remember_companions(runtime);
        let mut drawing = self.drawing.lock().expect("mod drawing fences");
        drawing.clear();
        let mut current = self.settings.lock().expect("mod settings snapshots");
        current.clear();
        let Some(runtime) = runtime else { return };
        for (index, metadata) in runtime.metadata.iter().enumerate() {
            let Some(registered) = &metadata.registration else {
                continue;
            };
            let host = runtime.hosts.host(index);
            if let Some(context) = super::admission::DrawingContext::capture(host) {
                drawing.insert(registered.entry.component.clone(), context);
            }
            if let Some(snapshot) = SettingsSnapshot::capture(host) {
                current.insert(registered.entry.component.clone(), snapshot);
            }
        }
    }
    /// Publishes a single instance's trap without revoking any other mod.
    pub(super) fn quarantine(&self, entry: &Entry, error: String) {
        self.send(Message::Failed {
            component: entry.component.clone(),
            reload: entry.reload,
            error,
        });
    }
    /// Retries acknowledgments without blocking the frame on the coordinator's mailbox.
    fn send(&self, message: Message) {
        if let Err(TrySendError::Full(message)) = self.messages.try_send(message) {
            self.acknowledgments
                .lock()
                .expect("mod acknowledgments")
                .push(message);
        }
    }
    /// Moves disposal to the loader so dropping an instance never joins a writer on-frame.
    pub(super) fn retire(&self, host: impl Into<Retired>) {
        let host = host.into();
        if let Err(error) = self.retired.try_send(host) {
            self.fallback_retired
                .lock()
                .expect("mod retirement")
                .push(error.into_inner());
        }
    }
    /// Counts hosts still waiting for bounded off-frame disposal capacity.
    fn retirement_used(&self) -> usize {
        self.retired.len() + self.fallback_retired.lock().expect("mod retirement").len()
    }
    /// Forwards hosts and acknowledgments deferred by a full bounded channel.
    fn retry(&self) {
        let pending =
            std::mem::take(&mut *self.acknowledgments.lock().expect("mod acknowledgments"));
        for message in pending {
            self.send(message);
        }
        let mut retired = self.fallback_retired.lock().expect("mod retirement");
        while let Some(host) = retired.pop() {
            if let Err(error) = self.retired.try_send(host) {
                retired.push(error.into_inner());
                break;
            }
        }
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.messages.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Applies complete transitions before raw input; admission never executes on this system.
pub(super) fn install_pending(mut commands: Commands, watcher: Res<Watcher>) {
    watcher.retry();
    commands.queue(install::sync_authority);
    let pending = watcher.pending.lock().expect("mod pending update").take();
    if let Some(update) = pending.or_else(|| watcher.updates.try_recv().ok()) {
        commands.queue(move |world: &mut World| install::apply(world, update));
    }
}

/// Drains all queued final saves before a disk-dependent load or deletion fence.
fn after_retirement<T, R>(retired: &Receiver<T>, build: impl FnOnce() -> R) -> R {
    for old in retired.try_iter() {
        drop(old);
    }
    build()
}
