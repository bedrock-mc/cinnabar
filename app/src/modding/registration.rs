//! Per-user extension registration; polling, compilation and status I/O stay off the frame thread.

#[cfg(test)]
mod tests;

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use assets::RuntimeFontCatalog;
use bevy::prelude::*;
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use mod_host::{ModGrants, ModHost};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ModRuntime, RELOAD_INTERVAL, VisualTimeOverride, font, interaction::ModInteraction};
use client_ui::ui_runtime::presentation::UiPresentationRuntime;

const REGISTRATION_FILE: &str = "local-mod.json";
const STATUS_FILE: &str = "local-mod.status.json";
const MAX_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BYTES: usize = 64;
const MAX_MESSAGE_BYTES: usize = 512;
static WRITE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    version: u32,
    #[serde(skip_serializing)]
    request_id: String,
    enabled: bool,
    component: PathBuf,
    #[serde(default)]
    font: Option<PathBuf>,
    #[serde(default)]
    grants: Grants,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct Grants {
    environment: bool,
    players: bool,
    camera: bool,
    controls: bool,
    interaction: bool,
    settings: bool,
}

impl From<&Grants> for ModGrants {
    fn from(grants: &Grants) -> Self {
        Self {
            environment: grants.environment,
            players: grants.players,
            camera: grants.camera,
            controls: grants.controls,
            interaction: grants.interaction,
            settings: grants.settings,
        }
    }
}

impl Registration {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported local extension registration version".into());
        }
        if self.request_id.is_empty()
            || self.request_id.len() > MAX_REQUEST_BYTES
            || self.request_id.chars().any(char::is_control)
        {
            return Err("registration request ID must be bounded plain text".into());
        }
        if !self.component.is_absolute()
            || self.font.as_ref().is_some_and(|font| !font.is_absolute())
        {
            return Err("registration component and font paths must be absolute".into());
        }
        Ok(())
    }
}

struct Candidate {
    host: ModHost,
    grants: ModGrants,
    font: Option<Arc<RuntimeFontCatalog>>,
    identity: [u8; 32],
    registration: Registration,
}

#[cfg(test)]
fn build_candidate(snapshot: SourceSnapshot) -> Result<Candidate, String> {
    build_candidate_with_settings(snapshot, None)
}

fn build_candidate_with_settings(
    snapshot: SourceSnapshot,
    settings: Option<&SettingsSnapshot>,
) -> Result<Candidate, String> {
    let SourceSnapshot {
        registration,
        identity,
        component,
        font: font_bytes,
    } = snapshot;
    let grants = ModGrants::from(&registration.grants);
    let font = if grants.controls {
        font_bytes
            .as_deref()
            .map(font::rasterize)
            .transpose()?
            .map(Arc::new)
    } else {
        None
    };
    let host = ModHost::prepare_snapshot_with_grants(
        &registration.component,
        &component,
        grants,
        settings.map(|settings| (settings.path.as_path(), settings.json.as_str())),
    )
    .map_err(|error| format!("{error:#}"))?;
    Ok(Candidate {
        host,
        grants,
        font,
        identity,
        registration,
    })
}

enum Action {
    Disabled,
    Replace(Box<Candidate>),
    Reuse {
        identity: [u8; 32],
        registration: Registration,
    },
}

struct Update {
    generation: u64,
    request_id: String,
    result: Result<Action, String>,
}

struct LoadRequest {
    generation: u64,
    snapshot: SourceSnapshot,
}

struct SourceSnapshot {
    registration: Registration,
    identity: [u8; 32],
    component: Vec<u8>,
    font: Option<Vec<u8>>,
}

enum Message {
    Acknowledge {
        generation: u64,
        status: Status,
        identity: Option<[u8; 32]>,
    },
    Reload {
        generation: u64,
        registration: Registration,
    },
    Stop,
}

#[derive(Serialize)]
struct Status {
    version: u32,
    request_id: String,
    client_pid: u32,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl Status {
    fn new(request_id: String, state: &'static str, message: Option<String>) -> Self {
        Self {
            version: 1,
            request_id,
            client_pid: std::process::id(),
            state,
            message: message.map(|message| bounded_message(&message)),
        }
    }
}

#[derive(Default)]
struct Desired {
    generation: u64,
    identity: Option<[u8; 32]>,
    request_id: String,
    status: Option<(&'static str, Option<String>)>,
}

struct WorkerLife(Arc<AtomicBool>);

impl Drop for WorkerLife {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
struct SettingsSnapshot {
    path: PathBuf,
    json: String,
}

#[derive(Resource)]
pub(super) struct Watcher {
    updates: Receiver<Update>,
    messages: Sender<Message>,
    retired: Sender<ModHost>,
    latest: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
    pending: Mutex<Option<Update>>,
    pending_ack: Mutex<Option<Message>>,
    desired: Arc<Mutex<Desired>>,
    alive: Arc<AtomicBool>,
    last_ack: AtomicU64,
    fallback_retired: Mutex<Vec<ModHost>>,
    settings: Arc<Mutex<Option<SettingsSnapshot>>>,
}

impl Watcher {
    pub(super) fn start(directory: PathBuf) -> Result<Self, String> {
        let (send_updates, updates) = bounded(1);
        let (messages, receive_messages) = bounded(8);
        let (retired, receive_retired) = bounded(2);
        let latest = Arc::new(AtomicU64::new(0));
        let generation = Arc::clone(&latest);
        let desired = Arc::new(Mutex::new(Desired::default()));
        let selection = Arc::clone(&desired);
        let alive = Arc::new(AtomicBool::new(true));
        let life = WorkerLife(Arc::clone(&alive));
        let settings = Arc::new(Mutex::new(None));
        let current_settings = Arc::clone(&settings);
        let thread = thread::Builder::new()
            .name("local-extension-registration".into())
            .spawn(move || {
                let _life = life;
                worker(
                    directory,
                    receive_messages,
                    send_updates,
                    receive_retired,
                    generation,
                    selection,
                    current_settings,
                )
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            updates,
            messages,
            retired,
            latest,
            thread: Some(thread),
            pending: Mutex::new(None),
            pending_ack: Mutex::new(None),
            desired,
            alive,
            last_ack: AtomicU64::new(0),
            fallback_retired: Mutex::new(Vec::with_capacity(2)),
            settings,
        })
    }

    fn retire(&self, host: ModHost) {
        // Main transitions reserve enough bounded retirement slots before removing a host.
        if let Err(error) = self.retired.try_send(host) {
            self.fallback_retired
                .lock()
                .expect("extension retirement mutex")
                .push(error.into_inner());
        }
    }

    pub(super) fn remember_settings(&self, host: Option<&ModHost>) {
        let mut current = self.settings.lock().expect("extension settings snapshot");
        let Some((path, json)) =
            host.and_then(|host| Some((host.settings_path()?, host.settings_snapshot()?)))
        else {
            *current = None;
            return;
        };
        if current
            .as_ref()
            .is_none_or(|current| current.path != path || current.json != json)
        {
            *current = Some(SettingsSnapshot {
                path: path.to_owned(),
                json: json.to_owned(),
            });
        }
    }

    fn acknowledge(&self, generation: u64, status: Status, identity: Option<[u8; 32]>) {
        self.last_ack.store(generation, Ordering::Release);
        self.send(Message::Acknowledge {
            generation,
            status,
            identity,
        });
    }

    pub(super) fn quarantine(&self, generation: u64, request_id: String, error: String) {
        let mut desired = self.desired.lock().expect("extension selection mutex");
        if generation != self.latest.load(Ordering::Acquire) {
            return;
        }
        *desired = Desired {
            generation,
            identity: None,
            request_id: request_id.clone(),
            status: Some(("error", Some(bounded_message(&error)))),
        };
        drop(desired);
        self.acknowledge(
            generation,
            Status::new(request_id, "error", Some(error)),
            None,
        );
    }

    fn send(&self, message: Message) {
        if let Err(TrySendError::Full(message)) = self.messages.try_send(message) {
            *self
                .pending_ack
                .lock()
                .expect("extension acknowledgment mutex") = Some(message);
        }
    }

    fn retirement_used(&self) -> usize {
        self.retired.len()
            + self
                .fallback_retired
                .lock()
                .expect("extension retirement mutex")
                .len()
    }

    fn retry_retirement(&self) {
        let mut pending = self
            .fallback_retired
            .lock()
            .expect("extension retirement mutex");
        while let Some(host) = pending.pop() {
            if let Err(error) = self.retired.try_send(host) {
                pending.push(error.into_inner());
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

/// Defers the complete host/font transition, then the schedule flushes it before raw input.
pub(super) fn install_pending(mut commands: Commands, watcher: Res<Watcher>) {
    watcher.retry_retirement();
    commands.queue(sync_authority);
    let acknowledgment = watcher
        .pending_ack
        .lock()
        .expect("extension acknowledgment mutex")
        .take();
    if let Some(message) = acknowledgment {
        watcher.send(message);
    }
    let pending = watcher
        .pending
        .lock()
        .expect("extension pending mutex")
        .take();
    if let Some(update) = pending.or_else(|| watcher.updates.try_recv().ok()) {
        commands.queue(move |world: &mut World| install(world, update));
        commands.queue(sync_authority);
    }
}

fn install(world: &mut World, update: Update) {
    let Some(watcher) = world.get_resource::<Watcher>() else {
        return;
    };
    // Serialize publication against watcher revocation, using only an in-memory lock.
    let selection = Arc::clone(&watcher.desired);
    let _authority = selection.lock().expect("extension selection mutex");
    if update.generation != watcher.latest.load(Ordering::Acquire)
        || !watcher.alive.load(Ordering::Acquire)
    {
        let required_slots = usize::from(matches!(&update.result, Ok(Action::Replace(_))));
        if watcher.retirement_used() + required_slots > watcher.retired.capacity().unwrap_or(0) {
            *watcher.pending.lock().expect("extension pending mutex") = Some(update);
            return;
        }
        if let Ok(Action::Replace(candidate)) = update.result {
            watcher.retire(candidate.host);
        }
        return;
    }
    if let Ok(Action::Reuse {
        identity,
        registration,
    }) = update.result
    {
        let active = world
            .get_resource_mut::<ModRuntime>()
            .is_some_and(|mut runtime| {
                if runtime.registration_identity != Some(identity)
                    || !runtime.host.is_active()
                    || runtime.suspended
                {
                    return false;
                }
                runtime.registration_request = Some((update.generation, update.request_id.clone()));
                true
            });
        if let Some(watcher) = world.get_resource::<Watcher>() {
            if active {
                watcher.acknowledge(
                    update.generation,
                    Status::new(update.request_id, "loaded", None),
                    Some(identity),
                );
            } else {
                watcher.send(Message::Reload {
                    generation: update.generation,
                    registration,
                });
            }
        }
        return;
    }
    if let Ok(Action::Replace(candidate)) = &update.result
        && world.get_resource::<ModRuntime>().is_some_and(|previous| {
            candidate.host.settings_path().is_some()
                && candidate.host.settings_path() == previous.host.settings_path()
                && candidate.host.settings_seed() != previous.host.settings_snapshot()
        })
    {
        let watcher = world.resource::<Watcher>();
        if watcher.retirement_used() >= watcher.retired.capacity().unwrap_or(0) {
            *watcher.pending.lock().expect("extension pending mutex") = Some(update);
            return;
        }
        if let Ok(Action::Replace(candidate)) = update.result {
            watcher.remember_settings(
                world
                    .get_resource::<ModRuntime>()
                    .map(|runtime| &runtime.host),
            );
            watcher.send(Message::Reload {
                generation: update.generation,
                registration: candidate.registration,
            });
            watcher.retire(candidate.host);
        }
        return;
    }
    // A replacement may retire both the current host and a rejected candidate.
    let watcher = world.resource::<Watcher>();
    let required_slots = usize::from(matches!(&update.result, Ok(Action::Replace(_))))
        + usize::from(world.contains_resource::<ModRuntime>());
    if watcher.retirement_used() + required_slots > watcher.retired.capacity().unwrap_or(0) {
        *watcher.pending.lock().expect("extension pending mutex") = Some(update);
        return;
    }
    let mut previous = clear_owned_state(world);
    let (state, message, identity) = match update.result {
        Ok(Action::Replace(mut candidate)) => {
            let installed = world
                .get_resource_mut::<UiPresentationRuntime>()
                .ok_or_else(|| "extension presentation is unavailable".to_owned())
                .and_then(|mut presentation| {
                    presentation
                        .set_mod_panel_font(candidate.font.clone())
                        .map_err(|error| error.to_string())?;
                    presentation.set_mod_label(candidate.host.label())?;
                    presentation.set_mod_panel(candidate.host.panel())?;
                    presentation.set_mod_panel_open(false);
                    Ok(())
                });
            match installed {
                Ok(()) => {
                    candidate.host.activate_settings(previous.as_mut());
                    world.insert_resource(VisualTimeOverride(candidate.host.time_override()));
                    world.insert_resource(ModInteraction::default());
                    world.insert_resource(ModRuntime {
                        host: candidate.host,
                        last_reload: Instant::now(),
                        grants: candidate.grants,
                        controls: mod_host::empty_controls(),
                        reload_on_main: false,
                        registration_identity: Some(candidate.identity),
                        registration_request: Some((update.generation, update.request_id.clone())),
                        suspended: false,
                    });
                    ("loaded", None, Some(candidate.identity))
                }
                Err(error) => {
                    clear_presentation(world);
                    if let Some(watcher) = world.get_resource::<Watcher>() {
                        watcher.retire(candidate.host);
                    }
                    ("error", Some(error), None)
                }
            }
        }
        Ok(Action::Disabled) => ("disabled", None, None),
        Err(error) => ("error", Some(error), None),
        Ok(Action::Reuse { .. }) => unreachable!("reuse is handled without replacing the host"),
    };
    if let Some(previous) = previous
        && let Some(watcher) = world.get_resource::<Watcher>()
    {
        watcher.retire(previous);
    }
    if let Some(watcher) = world.get_resource::<Watcher>() {
        watcher.acknowledge(
            update.generation,
            Status::new(update.request_id, state, message),
            identity,
        );
    }
}

/// Revocation releases frame authority independently of compilation or disk-bound disposal.
fn sync_authority(world: &mut World) {
    let Some(watcher) = world.get_resource::<Watcher>() else {
        return;
    };
    watcher.remember_settings(
        world
            .get_resource::<ModRuntime>()
            .map(|runtime| &runtime.host),
    );
    let desired = watcher.desired.lock().expect("extension selection mutex");
    let identity = watcher
        .alive
        .load(Ordering::Acquire)
        .then_some(desired.identity)
        .flatten();
    let acknowledgment = (desired.generation != 0
        && desired.generation != watcher.last_ack.load(Ordering::Acquire))
    .then(|| {
        desired.status.as_ref().map(|(state, message)| {
            (
                desired.generation,
                Status::new(desired.request_id.clone(), state, message.clone()),
            )
        })
    })
    .flatten();
    drop(desired);
    let suspend = world
        .get_resource_mut::<ModRuntime>()
        .is_some_and(|mut runtime| {
            if runtime.registration_identity == identity && !runtime.suspended {
                return false;
            }
            if runtime.suspended {
                return false;
            }
            runtime.suspended = true;
            runtime.controls = mod_host::empty_controls();
            runtime.host.set_panel_open(false);
            runtime.host.take_interaction();
            runtime.host.take_camera_delta();
            true
        });
    if suspend {
        if let Some(mut interaction) = world.get_resource_mut::<ModInteraction>() {
            *interaction = ModInteraction::default();
        }
        if let Some(mut time) = world.get_resource_mut::<VisualTimeOverride>() {
            time.0 = None;
        }
        clear_presentation(world);
    }
    if let Some((generation, status)) = acknowledgment
        && let Some(watcher) = world.get_resource::<Watcher>()
    {
        watcher.acknowledge(generation, status, None);
    }
}

fn clear_owned_state(world: &mut World) -> Option<ModHost> {
    let previous = world
        .remove_resource::<ModRuntime>()
        .map(|runtime| runtime.host);
    world.remove_resource::<VisualTimeOverride>();
    world.remove_resource::<ModInteraction>();
    clear_presentation(world);
    previous
}

fn clear_presentation(world: &mut World) {
    if let Some(mut presentation) = world.get_resource_mut::<UiPresentationRuntime>() {
        presentation.set_mod_panel_open(false);
        let _ = presentation.set_mod_panel(None);
        let _ = presentation.set_mod_label(None);
        if let Err(error) = presentation.set_mod_panel_font(None) {
            eprintln!("Personal-panel font could not be released: {error}");
        }
    }
}

struct Observation {
    fingerprint: [u8; 32],
    request_id: Option<String>,
    result: Result<Option<Registration>, String>,
}

fn observe(path: &Path) -> Observation {
    let bytes = (|| {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(Some(bytes))
    })();
    match bytes {
        Ok(None) => Observation {
            fingerprint: [0; 32],
            request_id: None,
            result: Ok(None),
        },
        Err(error) => Observation {
            fingerprint: Sha256::digest(error.as_bytes()).into(),
            request_id: None,
            result: Err(format!("registration could not be read: {error}")),
        },
        Ok(Some(bytes)) => {
            let request_id = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("request_id")
                        .and_then(|id| id.as_str())
                        .map(str::to_owned)
                })
                .filter(|id| id.len() <= MAX_REQUEST_BYTES && !id.chars().any(char::is_control));
            let result = if bytes.len() > MAX_BYTES {
                Err("local extension registration exceeds byte limit".into())
            } else {
                serde_json::from_slice::<Registration>(&bytes)
                    .map_err(|error| format!("invalid local extension registration: {error}"))
                    .and_then(|registration| registration.validate().map(|()| Some(registration)))
            };
            Observation {
                fingerprint: Sha256::digest(&bytes).into(),
                request_id,
                result,
            }
        }
    }
}

fn source_snapshot(registration: Registration) -> Result<SourceSnapshot, String> {
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(&registration).map_err(|error| error.to_string())?);
    let component = read_bounded_source(&registration.component, mod_host::MAX_COMPONENT_BYTES)?;
    digest.update(Sha256::digest(&component));
    let font = if registration.grants.controls {
        registration
            .font
            .as_deref()
            .map(|path| read_bounded_source(path, font::MAX_SOURCE_BYTES))
            .transpose()?
    } else {
        None
    };
    if let Some(font) = &font {
        digest.update(Sha256::digest(font));
    }
    Ok(SourceSnapshot {
        registration,
        identity: digest.finalize().into(),
        component,
        font,
    })
}

fn read_bounded_source(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take((max + 1) as u64).read_to_end(&mut bytes))
        .map_err(|error| {
            format!(
                "extension source {} could not be read: {error}",
                path.display()
            )
        })?;
    if bytes.len() > max {
        return Err("extension source exceeds byte limit".into());
    }
    Ok(bytes)
}

fn after_retirement<T, R>(retired: &Receiver<T>, build: impl FnOnce() -> R) -> R {
    for old in retired.try_iter() {
        drop(old);
    }
    build()
}

fn worker(
    directory: PathBuf,
    messages: Receiver<Message>,
    updates: Sender<Update>,
    receive_retired: Receiver<ModHost>,
    latest: Arc<AtomicU64>,
    desired: Arc<Mutex<Desired>>,
    settings: Arc<Mutex<Option<SettingsSnapshot>>>,
) {
    let (loads, receive_loads) = bounded::<LoadRequest>(1);
    let (results, receive_results) = bounded(1);
    let generation = Arc::clone(&latest);
    let loader = thread::Builder::new()
        .name("local-extension-loader".into())
        .spawn(move || {
            loop {
                crossbeam_channel::select! {
                    recv(receive_loads) -> request => {
                        let Ok(request) = request else { break };
                        if request.generation != generation.load(Ordering::Acquire) { continue; }
                        let request_id = request.snapshot.registration.request_id.clone();
                        let result = after_retirement(&receive_retired, || {
                            let current = settings.lock().expect("extension settings snapshot").clone();
                            build_candidate_with_settings(request.snapshot, current.as_ref())
                                .map(|candidate| Action::Replace(Box::new(candidate)))
                        });
                        if request.generation == generation.load(Ordering::Acquire) {
                            let _ = results.send(Update {
                                generation: request.generation,
                                request_id,
                                result,
                            });
                        }
                    }
                    recv(receive_retired) -> host => {
                        let Ok(host) = host else { break };
                        drop(host);
                    }
                }
            }
        });
    let Ok(loader) = loader else {
        return;
    };
    let mut fingerprint = None;
    let mut request_id = String::new();
    let mut installed_identity = None;
    let mut pending_load = None;
    let mut pending_update = None;
    let mut next_poll = Instant::now();
    loop {
        if Instant::now() >= next_poll {
            next_poll = Instant::now() + RELOAD_INTERVAL;
            let mut observed = observe(&directory.join(REGISTRATION_FILE));
            let mut snapshot = match &observed.result {
                Ok(Some(registration)) if registration.enabled => {
                    match source_snapshot(registration.clone()) {
                        Ok(snapshot) => Some(snapshot),
                        Err(error) => {
                            observed.result = Err(error);
                            None
                        }
                    }
                }
                _ => None,
            };
            let mut digest = Sha256::new();
            digest.update(observed.fingerprint);
            if let Some(snapshot) = &snapshot {
                digest.update(snapshot.identity);
            }
            if let Err(error) = &observed.result {
                digest.update(error.as_bytes());
            }
            observed.fingerprint = digest.finalize().into();
            if fingerprint != Some(observed.fingerprint) {
                let initial_absence = fingerprint.is_none() && matches!(observed.result, Ok(None));
                fingerprint = Some(observed.fingerprint);
                if let Some(id) = observed.request_id {
                    request_id = id;
                }
                if !initial_absence {
                    let status = match &observed.result {
                        Ok(Some(registration)) if registration.enabled => None,
                        Ok(_) => Some(("disabled", None)),
                        Err(error) => Some(("error", Some(bounded_message(error)))),
                    };
                    let mut selection = desired.lock().expect("extension selection mutex");
                    let generation = latest.fetch_add(1, Ordering::AcqRel) + 1;
                    *selection = Desired {
                        generation,
                        identity: snapshot.as_ref().map(|snapshot| snapshot.identity),
                        request_id: request_id.clone(),
                        status,
                    };
                    drop(selection);
                    pending_load = None;
                    pending_update = None;
                    match observed.result {
                        Ok(Some(registration)) if registration.enabled => {
                            let snapshot =
                                snapshot.take().expect("enabled source was validated above");
                            let identity = snapshot.identity;
                            if installed_identity == Some(identity) {
                                pending_update = Some(Update {
                                    generation,
                                    request_id: request_id.clone(),
                                    result: Ok(Action::Reuse {
                                        identity,
                                        registration,
                                    }),
                                });
                            } else {
                                pending_load = Some(LoadRequest {
                                    generation,
                                    snapshot,
                                });
                            }
                        }
                        result => {
                            pending_update = Some(Update {
                                generation,
                                request_id: request_id.clone(),
                                result: result.map(|_| Action::Disabled),
                            });
                        }
                    }
                }
            }
        }
        if let Some(request) = pending_load.take()
            && let Err(TrySendError::Full(request)) = loads.try_send(request)
        {
            pending_load = Some(request);
        }
        for update in receive_results.try_iter() {
            if update.generation == latest.load(Ordering::Acquire) {
                if let Err(error) = &update.result {
                    *desired.lock().expect("extension selection mutex") = Desired {
                        generation: update.generation,
                        identity: None,
                        request_id: update.request_id.clone(),
                        status: Some(("error", Some(bounded_message(error)))),
                    };
                }
                pending_update = Some(update);
            }
        }
        if let Some(update) = pending_update.take()
            && let Err(TrySendError::Full(update)) = updates.try_send(update)
        {
            pending_update = Some(update);
        }
        match messages.recv_timeout(next_poll.saturating_duration_since(Instant::now())) {
            Ok(Message::Acknowledge {
                generation,
                status,
                identity,
            }) => {
                if generation == latest.load(Ordering::Acquire) {
                    installed_identity = identity;
                    if let Err(error) = write_status(&directory.join(STATUS_FILE), &status) {
                        eprintln!("Local extension status could not be saved: {error}");
                    }
                }
            }
            Ok(Message::Reload {
                generation,
                registration,
            }) => {
                if generation == latest.load(Ordering::Acquire) {
                    match source_snapshot(registration) {
                        Ok(snapshot) => {
                            if desired.lock().expect("extension selection mutex").identity
                                != Some(snapshot.identity)
                            {
                                next_poll = Instant::now();
                                continue;
                            }
                            pending_load = Some(LoadRequest {
                                generation,
                                snapshot,
                            })
                        }
                        Err(error) => {
                            *desired.lock().expect("registration authority") = Desired {
                                generation,
                                identity: None,
                                request_id: request_id.clone(),
                                status: Some(("error", Some(bounded_message(&error)))),
                            };
                            pending_update = Some(Update {
                                generation,
                                request_id: request_id.clone(),
                                result: Err(error),
                            })
                        }
                    }
                }
            }
            Ok(Message::Stop) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
    latest.fetch_add(1, Ordering::AcqRel);
    drop(receive_results);
    drop(loads);
    let _ = loader.join();
}

fn bounded_message(message: &str) -> String {
    let mut output = String::new();
    for character in message.chars().filter(|character| !character.is_control()) {
        if output.len() + character.len_utf8() > MAX_MESSAGE_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

fn write_status(path: &Path, status: &Status) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(status)?;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("status directory unavailable"))?;
    fs::create_dir_all(parent)?;
    let id = WRITE_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("pending-{}-{id}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    let _ = fs::remove_file(temporary);
    result
}
