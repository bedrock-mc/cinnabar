//! Per-user extension registration; polling, compilation and status I/O stay off the frame thread.

use sha2::Digest;
mod presentation;
mod source;
mod status;
#[cfg(test)]
mod tests;

use presentation::{clear_owned_state, clear_presentation, refresh_presentation};
use source::{observe, source_snapshot};
use status::{bounded_message, write_status};

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Instant,
};

use assets::RuntimeFontCatalog;
use bevy::prelude::*;
use client_extensions::modding::{font, multi::ModHosts};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use mod_host::{ModGrants, ModHost};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::{ModRuntime, RELOAD_INTERVAL, VisualTimeOverride, interaction::ModInteraction};
use client_shell::ui_resources::UiPresentationResource;

const REGISTRATION_FILE: &str = "local-mod.json";
const STATUS_FILE: &str = "local-mod.status.json";
const MAX_BYTES: usize = 16 * 1024;
const MAX_REQUEST_BYTES: usize = 64;

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
    ui: Option<PathBuf>,
    #[serde(default)]
    grants: ModGrants,
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
            || self.ui.as_ref().is_some_and(|ui| !ui.is_absolute())
        {
            return Err("registration component, font and UI paths must be absolute".into());
        }
        Ok(())
    }
}

struct Candidate {
    host: ModHost,
    font: Option<Arc<RuntimeFontCatalog>>,
    panel_ui: Option<Arc<mod_ui::PreparedPanelUi>>,
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
        ui,
    } = snapshot;
    let grants = registration.grants.clone();
    let font = if grants.controls {
        font_bytes
            .as_deref()
            .map(font::rasterize)
            .transpose()?
            .map(Arc::new)
    } else {
        None
    };
    let mut host = ModHost::prepare_snapshot_with_grants(
        &registration.component,
        &component,
        grants,
        settings.map(|settings| (settings.path.as_path(), settings.json.as_str())),
    )
    .map_err(|error| format!("{error:#}"))?;
    if let Some(ui) = ui {
        let json = std::str::from_utf8(&ui).map_err(|error| error.to_string())?;
        host.set_panel_bundle(json)
            .map_err(|error| format!("{error:#}"))?;
    }
    let panel_ui = host
        .panel_bundle()
        .map(mod_ui::PreparedPanelUi::prepare)
        .transpose()?;
    Ok(Candidate {
        host,
        font,
        panel_ui,
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
    ui: Option<Vec<u8>>,
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
    independent_registration: bool,
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
            independent_registration: true,
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
                    || !runtime
                        .registration_host
                        .is_some_and(|index| runtime.hosts.host(index).is_active())
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
                && candidate.host.settings_path()
                    == previous
                        .registration_host
                        .and_then(|index| previous.hosts.host(index).settings_path())
                && candidate.host.settings_seed()
                    != previous
                        .registration_host
                        .and_then(|index| previous.hosts.host(index).settings_snapshot())
        })
    {
        let watcher = world.resource::<Watcher>();
        if watcher.retirement_used() >= watcher.retired.capacity().unwrap_or(0) {
            *watcher.pending.lock().expect("extension pending mutex") = Some(update);
            return;
        }
        if let Ok(Action::Replace(candidate)) = update.result {
            watcher.remember_settings(world.get_resource::<ModRuntime>().and_then(|runtime| {
                runtime
                    .registration_host
                    .map(|index| runtime.hosts.host(index))
            }));
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
        + usize::from(
            world
                .get_resource::<ModRuntime>()
                .is_some_and(|runtime| runtime.registration_host.is_some()),
        );
    if watcher.retirement_used() + required_slots > watcher.retired.capacity().unwrap_or(0) {
        *watcher.pending.lock().expect("extension pending mutex") = Some(update);
        return;
    }
    let mut previous = clear_owned_state(world);
    let (state, message, identity) = match update.result {
        Ok(Action::Replace(mut candidate)) => {
            let full = world
                .get_resource::<ModRuntime>()
                .is_some_and(|runtime| runtime.hosts.host_count() >= mod_api::MAX_LOADED_MODS);
            let installed = (!full)
                .then_some(())
                .ok_or_else(|| "local extension limit reached".to_owned())
                .and_then(|()| {
                    world
                        .get_resource_mut::<UiPresentationResource>()
                        .ok_or_else(|| "extension presentation is unavailable".to_owned())
                        .and_then(|mut presentation| {
                            presentation
                                .mod_ui_mut()
                                .set_mod_label(candidate.host.label())?;
                            presentation.set_mod_hud(candidate.host.hud())?;
                            presentation
                                .mod_ui_mut()
                                .set_mod_crosshair(candidate.host.crosshair())?;
                            presentation.mod_ui_mut().set_prepared_panel_ui(
                                mod_ui::PanelUiOwner::Registration,
                                candidate.panel_ui.clone(),
                            );
                            if let Some(panel) = candidate.host.panel() {
                                panel.validate_values()?;
                            }
                            Ok(())
                        })
                });
            match installed {
                Ok(()) => {
                    candidate.host.activate_settings(previous.as_mut());
                    if !world.contains_resource::<ModRuntime>() {
                        world.insert_resource(VisualTimeOverride(candidate.host.time_override()));
                        world.insert_resource(ModInteraction::default());
                    }
                    if let Some(mut runtime) = world.get_resource_mut::<ModRuntime>() {
                        runtime.render_sources.clear();
                        let index = runtime.hosts.host_count();
                        runtime.panel_fonts.push(candidate.font.clone());
                        runtime.hosts.companions.push(
                            client_extensions::modding::multi::Companion {
                                host: candidate.host,
                            },
                        );
                        runtime.registration_host = Some(index);
                        runtime.registration_identity = Some(candidate.identity);
                        runtime.registration_request =
                            Some((update.generation, update.request_id.clone()));
                    } else {
                        world.insert_resource(ModRuntime {
                            panel_fonts: vec![candidate.font.clone()],
                            hosts: ModHosts::new(candidate.host, Vec::new()),
                            render_sources: Vec::new(),
                            render_merge: Default::default(),
                            last_reload: Instant::now(),
                            controls: mod_host::empty_controls(),
                            registration_identity: Some(candidate.identity),
                            registration_host: Some(0),
                            registration_request: Some((
                                update.generation,
                                update.request_id.clone(),
                            )),
                            suspended: false,
                            hud_editor_owner: None,
                            screens: Default::default(),
                        });
                    }
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
    refresh_presentation(world);
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
    watcher.remember_settings(world.get_resource::<ModRuntime>().and_then(|runtime| {
        runtime
            .registration_host
            .map(|index| runtime.hosts.host(index))
    }));
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
            let Some(index) = runtime.registration_host else {
                return false;
            };
            if index > 0 {
                runtime.hosts.host_mut(index).quarantine();
                return false;
            }
            runtime.suspended = true;
            runtime.controls = mod_host::empty_controls();
            runtime.hosts.host.set_panel_open(false);
            runtime.hosts.host.take_interaction();
            runtime.hosts.host.take_camera_delta();
            runtime.hosts.host.take_commands();
            runtime.hosts.host.take_cues();
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
    if world
        .get_resource::<ModRuntime>()
        .is_some_and(|runtime| !runtime.suspended)
    {
        refresh_presentation(world);
    }
    if let Some((generation, status)) = acknowledgment
        && let Some(watcher) = world.get_resource::<Watcher>()
    {
        watcher.acknowledge(generation, status, None);
    }
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
