use std::{
    sync::{Arc, Condvar, Mutex, TryLockError},
    thread,
    time::{Duration, Instant},
};

use bridge::{Backend, LocalTarget, LocalWorld, LocalWorldInfo, Transport, WorldState};
use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError};
use launcher::local_worlds::{
    in_world::{Applied, Timing, timing},
    options::OptionChanges,
    world::{
        NATURAL_TERRAIN_REQUIRES_BDS, NewWorld, Prefs, PrefsUpdate, SetupState, World, WorldStatus,
        WorldUpdate,
    },
};

use crate::lifecycle::children::{self, ExitHookGuard};

use super::{
    Cancel, CancelHandle, cancel_pair, lock,
    process::{HostGrant, Instance, Runner, SAVE_BUDGET, StartSpec},
    provision::{Provisioner, RuntimeInfo, RuntimeKind},
    store::Store,
    world::{WorldError, validate_name},
};

const DEFAULT_STOP_TIMEOUT: Duration = SAVE_BUDGET;
/// Outlasts the Docker probe's own timeout.
const DEFAULT_RUNTIME_WAIT: Duration = Duration::from_secs(25);
/// Covers a first sign-in's profile load, counted from before the server launches; a start then
/// proceeds without an operator.
const DEFAULT_HOST_WAIT: Duration = Duration::from_secs(30);
/// How often a start waiting on the host rechecks its cancel.
const HOST_POLL: Duration = Duration::from_millis(50);

/// The account hosting later opens, which BDS makes an operator as a vanilla world owner is.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Host {
    #[default]
    SignedOut,
    /// Signed in with the XUID still loading; BDS starts wait for it before reporting running.
    Loading,
    Xuid(String),
}

/// The runner of each backend; a missing one cannot host worlds.
#[derive(Clone, Default)]
pub(crate) struct Runners {
    pub bds: Option<Arc<dyn Runner>>,
    pub dragonfly: Option<Arc<dyn Runner>>,
}

impl Runners {
    fn start(&self, spec: &StartSpec, cancel: &Cancel) -> Result<Arc<dyn Instance>, WorldError> {
        let runner = match spec.world.backend {
            Backend::Bds => &self.bds,
            Backend::Dragonfly => &self.dragonfly,
        };
        runner
            .as_ref()
            .ok_or(WorldError::BackendUnavailable)?
            .start(spec, cancel)
    }

    /// When `backend`'s runner applies the host grant; Dragonfly and a missing runner grant nobody.
    fn host_grant(&self, backend: Backend) -> HostGrant {
        match (backend, &self.bds) {
            (Backend::Bds, Some(runner)) => runner.host_grant(),
            (Backend::Bds, None) | (Backend::Dragonfly, _) => HostGrant::None,
        }
    }
}

/// What a [`Manager`] is built from.
pub(crate) struct Parts {
    pub store: Arc<Store>,
    pub runners: Runners,
    /// The dedicated-server installer whose status is reported and whose EULA gates BDS worlds.
    pub setup: Option<Arc<Provisioner>>,
    /// Lets a Docker re-probe change the default backend of new worlds.
    pub auto_backend: bool,
    /// Backends that cannot host worlds here, with the reason for the log.
    pub unavailable: Vec<(Backend, String)>,
    pub stop_timeout: Duration,
    /// How long a create waits for a runtime detection in flight.
    pub runtime_wait: Duration,
    /// How long a BDS start waits for a [`Host::Loading`] host, counted from before its launch.
    pub host_wait: Duration,
}

impl Parts {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            runners: Runners::default(),
            setup: None,
            auto_backend: false,
            unavailable: Vec::new(),
            stop_timeout: DEFAULT_STOP_TIMEOUT,
            runtime_wait: DEFAULT_RUNTIME_WAIT,
            host_wait: DEFAULT_HOST_WAIT,
        }
    }
}

/// Prefer the dedicated server wherever a complete runtime is available.
pub(crate) fn default_backend(runtime: &RuntimeInfo) -> Backend {
    match runtime.kind {
        RuntimeKind::Native | RuntimeKind::Container | RuntimeKind::Translated => Backend::Bds,
        RuntimeKind::None => Backend::Dragonfly,
    }
}

/// Owns the saved worlds and at most one running local server; clones share it.
#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<Store>,
    runners: Runners,
    setup: Option<Arc<Provisioner>>,
    unavailable: Vec<(Backend, String)>,
    stop_timeout: Duration,
    runtime_wait: Duration,
    life: Mutex<Life>,
    host_wait: Duration,
    /// Read by each start; see [`Manager::set_host`].
    host: Mutex<Host>,
    host_changed: Condvar,
    /// Starts that have waited on a loading host, so tests can publish it mid-wait.
    #[cfg(test)]
    host_waits: std::sync::atomic::AtomicUsize,
    subscribers: Mutex<Vec<Sender<()>>>,
    /// Background starts, stops and exit watches still running.
    background: Mutex<usize>,
    background_done: Condvar,
    /// Asks the running server to stop its own way when the client exits without destructors.
    _exit: ExitHookGuard<'static>,
}

/// The single open world's lifecycle.
#[derive(Default)]
struct Life {
    state: WorldState,
    world: Option<World>,
    instance: Option<Arc<dyn Instance>>,
    paused: bool,
    failure: Option<String>,
    /// Cancels the start in progress when dropped.
    cancel_start: Option<CancelHandle>,
    /// Disconnects once the start in progress has finished, including stopping a cancelled server.
    start_done: Option<Receiver<()>>,
    /// Servers still saving or cleaning up after a close or exit; an open waits for all of them, so no
    /// world's files or backend resources are shared with a new server.
    retiring: Vec<Retiring>,
    /// Counts opens, so a stop that finishes late never idles a newer open.
    generation: u64,
    /// Pending edits applied live by one server generation; only that server can save them.
    live_applied: Option<(u64, OptionChanges)>,
}

/// A world whose exited or closing server has not finished backend cleanup.
#[derive(Clone)]
struct Retiring {
    world_id: String,
    /// Disconnects once the server has exited and its backend cleanup has finished.
    done: Receiver<()>,
}

impl Retiring {
    fn done(&self) -> bool {
        matches!(self.done.try_recv(), Err(TryRecvError::Disconnected))
    }
}

impl Manager {
    pub(crate) fn from_parts(parts: Parts) -> Self {
        if let Some(setup) = &parts.setup {
            let store = Arc::clone(&parts.store);
            let auto = parts.auto_backend;
            let unavailable = parts.unavailable.clone();
            setup.on_detected(Arc::new(move |info: &RuntimeInfo| {
                tracing::info!(
                    "local world runtime detected: {} ({})",
                    info.kind.as_str(),
                    info.reason
                );
                if auto {
                    let backend = default_backend(info);
                    store.set_default_backend(backend);
                    if let Some((_, reason)) =
                        unavailable.iter().find(|(known, _)| *known == backend)
                    {
                        tracing::error!("the default local world backend is unavailable: {reason}");
                    }
                }
            }));
        }
        Self {
            inner: Arc::new_cyclic(|inner| Inner {
                _exit: {
                    let inner = inner.clone();
                    children::on_exit(move || {
                        if let Some(inner) = inner.upgrade() {
                            Self { inner }.request_exit_stop();
                        }
                    })
                },
                store: parts.store,
                runners: parts.runners,
                setup: parts.setup,
                unavailable: parts.unavailable,
                stop_timeout: parts.stop_timeout,
                runtime_wait: parts.runtime_wait,
                life: Mutex::default(),
                host_wait: parts.host_wait,
                host: Mutex::default(),
                host_changed: Condvar::new(),
                #[cfg(test)]
                host_waits: Default::default(),
                subscribers: Mutex::default(),
                background: Mutex::new(0),
                background_done: Condvar::new(),
            }),
        }
    }

    /// Deletes imports a killed client left behind; it walks and deletes folders, so it runs on
    /// the worlds worker.
    pub fn reclaim_abandoned_imports(&self) {
        self.inner.store.reclaim_abandoned_imports();
    }

    /// Saved worlds after pending server saves finish, most recently played first.
    pub fn list(&self) -> Result<Vec<World>, WorldError> {
        let after: Vec<_> = lock(&self.inner.life)
            .retiring
            .iter()
            .map(|retiring| retiring.done.clone())
            .collect();
        for done in after {
            let _ = done.recv();
        }
        self.inner.store.list()
    }

    /// World `id`'s saved menu thumbnail, if one was captured.
    #[must_use]
    pub fn thumbnail(&self, id: &str) -> Option<Vec<u8>> {
        self.inner.store.thumbnail(id)
    }

    /// Saves `image` as world `id`'s menu thumbnail.
    pub fn save_thumbnail(&self, id: &str, image: &[u8]) -> Result<(), WorldError> {
        self.inner.store.save_thumbnail(id, image)
    }

    /// Saves a new world; a BDS world is refused where BDS cannot run. Unless Dragonfly was asked
    /// for, it first waits out a runtime detection in flight, so no world is saved against a guess.
    pub fn create(&self, world: &NewWorld) -> Result<World, WorldError> {
        let mut world = world.clone();
        world.name = validate_name(&world.name)?;
        let backend = self.creatable_backend(world.backend)?;
        if !world.generator.supports_backend(backend) {
            return Err(WorldError::Invalid(NATURAL_TERRAIN_REQUIRES_BDS.to_owned()));
        }
        world.backend = Some(backend);
        self.inner.store.create(&world)
    }

    /// Imports the `.mcworld` at `archive` as a new world on this machine's default backend.
    pub fn import(
        &self,
        archive: &std::path::Path,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<World, WorldError> {
        let backend = self.creatable_backend(None)?;
        super::import::import(
            &self.inner.store,
            archive,
            backend,
            &super::import::WORLD_LIMITS,
            cancelled,
        )
    }

    /// The backend a new world runs on, or why none can be saved yet.
    fn creatable_backend(&self, requested: Option<Backend>) -> Result<Backend, WorldError> {
        if let Some(setup) = &self.inner.setup
            && requested != Some(Backend::Dragonfly)
            && !setup.await_runtime(self.inner.runtime_wait)
        {
            return Err(WorldError::RuntimePending);
        }
        let backend = requested.unwrap_or_else(|| self.inner.store.default_backend());
        if let Some((_, reason)) = self
            .inner
            .unavailable
            .iter()
            .find(|(known, _)| *known == backend)
        {
            tracing::error!("cannot create a local world: {reason}");
            return Err(WorldError::BackendUnavailable);
        }
        if backend == Backend::Bds && !self.bds_runnable() {
            return Err(WorldError::BackendUnavailable);
        }
        Ok(backend)
    }

    fn bds_runnable(&self) -> bool {
        self.inner
            .setup
            .as_ref()
            .is_some_and(|setup| setup.status().0.state != SetupState::Unsupported)
    }

    /// Renames a world or changes its game mode or difficulty, which apply on its next open.
    pub fn update(&self, id: &str, update: &WorldUpdate) -> Result<World, WorldError> {
        self.inner.store.update(id, update)
    }

    /// Saves `update` to world `id` and sends the running server the commands that apply it at
    /// once; settings the server reads only at startup, or that it failed to take, apply on the
    /// next start.
    pub fn apply(&self, id: &str, update: &WorldUpdate) -> Result<Applied, WorldError> {
        let world = self.inner.store.update(id, update)?;
        let (instance, generation) = {
            let mut life = lock(&self.inner.life);
            let running = life.state == WorldState::Running
                && life.world.as_ref().is_some_and(|open| open.id == id);
            if !running {
                return Ok(Applied {
                    world,
                    missed: false,
                });
            }
            life.world = Some(world.clone());
            self.notify_subscribers();
            (life.instance.clone(), life.generation)
        };
        let Some(instance) = instance else {
            return Ok(Applied {
                world,
                missed: false,
            });
        };
        let commands = super::bds::live_commands(&world, update);
        let sent = commands
            .iter()
            .all(|command| match instance.console(command) {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!("local world {id}: `{command}` failed: {error}");
                    false
                }
            });
        if sent && let Some(changes) = &update.options {
            let mut life = lock(&self.inner.life);
            if life.generation == generation
                && life.state == WorldState::Running
                && life
                    .instance
                    .as_ref()
                    .is_some_and(|running| Arc::ptr_eq(running, &instance))
            {
                life.live_applied
                    .get_or_insert_with(|| (generation, OptionChanges::default()))
                    .1
                    .extend(&changes.filtered(|id| timing(id) == Timing::Live));
            }
        }
        Ok(Applied {
            world,
            missed: !sent,
        })
    }

    /// Deletes a world that is not starting, running or stopping.
    pub fn delete(&self, id: &str) -> Result<(), WorldError> {
        let life = lock(&self.inner.life);
        let open = !matches!(life.state, WorldState::Idle | WorldState::Failed);
        let saving = life
            .retiring
            .iter()
            .any(|retiring| retiring.world_id == id && !retiring.done());
        if saving || (open && life.world.as_ref().is_some_and(|world| world.id == id)) {
            return Err(WorldError::InUse);
        }
        self.inner.store.delete(id)
    }

    /// Records the user's acceptance of the Minecraft EULA, so BDS worlds may download the server.
    pub fn accept_eula(&self) -> Result<(), WorldError> {
        self.inner
            .setup
            .as_ref()
            .ok_or(WorldError::BackendUnavailable)?
            .accept_eula()
    }

    /// Applies `update`, re-probing Docker first when asked, and returns the saved preferences. A
    /// re-probe only changes the backend of worlds created afterwards.
    pub fn prefs(&self, update: PrefsUpdate) -> Result<Prefs, WorldError> {
        if update.redetect
            && let Some(setup) = &self.inner.setup
        {
            setup.redetect();
        }
        match update.docker_prompt_dismissed {
            Some(dismissed) => self.inner.store.update_prefs(dismissed),
            None => Ok(self.inner.store.prefs()),
        }
    }

    /// The open world's lifecycle and the dedicated server's setup.
    pub fn status(&self) -> WorldStatus {
        let mut status = {
            let life = lock(&self.inner.life);
            WorldStatus {
                state: life.state,
                world_id: life.world.as_ref().map(|world| world.id.clone()),
                backend: life.world.as_ref().map(|world| world.backend),
                paused: life.paused,
                pause_supported: life
                    .instance
                    .as_ref()
                    .is_none_or(|instance| instance.can_pause()),
                error: life.failure.clone(),
                setup: None,
                backend_unavailable_reason: None,
                max_players: life
                    .instance
                    .as_ref()
                    .and_then(|instance| instance.max_players()),
            }
        };
        if let Some(setup) = &self.inner.setup {
            let (setup, unavailable) = setup.status();
            status.setup = Some(setup);
            status.backend_unavailable_reason = unavailable;
        }
        status
    }

    /// Readies a Dragonfly server ahead of an open, while the worlds are on screen.
    pub fn prepare(&self) {
        if let Some(runner) = &self.inner.runners.dragonfly {
            runner.prepare();
        }
    }

    /// Begins starting a world and returns at once; poll [`Self::status`] for readiness.
    /// Reopening the world that is already starting or running does nothing, and an open while
    /// the last world still saves starts its server once that one has exited.
    pub fn open(&self, id: &str, view_distance: Option<u32>) -> Result<(), WorldError> {
        let mut life = lock(&self.inner.life);
        match life.state {
            WorldState::Starting | WorldState::Running => {
                return if life.world.as_ref().is_some_and(|world| world.id == id) {
                    Ok(())
                } else {
                    Err(WorldError::Busy)
                };
            }
            WorldState::Idle | WorldState::Stopping | WorldState::Failed => {}
        }
        let world = self.inner.store.get(id)?;
        let dir = self.inner.store.dir(id)?;
        if world.backend == Backend::Bds {
            let setup = self
                .inner
                .setup
                .as_ref()
                .ok_or(WorldError::BackendUnavailable)?
                .status()
                .0;
            if setup.state == SetupState::Unsupported {
                return Err(WorldError::BackendUnavailable);
            }
            if !setup.eula_accepted {
                return Err(WorldError::EulaRequired);
            }
        }
        let (handle, cancel) = cancel_pair();
        let (start_running, start_done) = crossbeam_channel::bounded::<()>(0);
        life.retiring.retain(|retiring| !retiring.done());
        let after: Vec<Receiver<()>> = life
            .retiring
            .iter()
            .map(|retiring| retiring.done.clone())
            .collect();
        life.generation += 1;
        life.live_applied = None;
        let generation = life.generation;
        life.world = Some(world.clone());
        life.instance = None;
        life.failure = None;
        life.paused = false;
        life.cancel_start = Some(handle);
        life.start_done = Some(start_done);
        self.set_state(&mut life, WorldState::Starting);
        let spec = StartSpec {
            world,
            dir,
            view_distance,
            options_unapplied: false,
            host_xuid: None,
        };
        let started = self.background(move |manager| {
            manager.start(&spec, &cancel, generation, after);
            drop(start_running);
        });
        if !started {
            life.failure = Some(WorldError::Cancelled.status_text());
            self.set_state(&mut life, WorldState::Failed);
        }
        Ok(())
    }

    /// Starts `spec`'s server once every server in `after` has finished stopping.
    fn start(&self, spec: &StartSpec, cancel: &Cancel, generation: u64, after: Vec<Receiver<()>>) {
        for retiring in after {
            let cancelled = crossbeam_channel::select! {
                recv(retiring) -> _ => false,
                recv(cancel.receiver()) -> _ => true,
            };
            if cancelled {
                break;
            }
        }
        let grant = self.inner.runners.host_grant(spec.world.backend);
        // Set before launch, so a new world's creation run counts against the host's wait.
        let host_deadline = Instant::now() + self.inner.host_wait;
        let host_xuid = match grant {
            HostGrant::None => None,
            HostGrant::AtLaunch => self.await_host(host_deadline, cancel, "before launch", None),
            HostGrant::WhenReady => self.known_host(),
        };
        let mut fresh_world = None;
        let mut applied = Default::default();
        let started = if cancel.is_cancelled() {
            Err(WorldError::Cancelled)
        } else {
            self.inner.store.snapshot(&spec.world.id).and_then(
                |(world, pending, options_unapplied)| {
                    applied = pending;
                    let fresh = StartSpec {
                        world,
                        dir: spec.dir.clone(),
                        view_distance: spec.view_distance,
                        options_unapplied,
                        host_xuid: host_xuid.clone(),
                    };
                    let started = self.inner.runners.start(&fresh, cancel);
                    fresh_world = Some(fresh.world);
                    started
                },
            )
        };
        let (started, host_guard) = match started {
            Ok(instance) if grant == HostGrant::WhenReady && !cancel.is_cancelled() => {
                match self.grant_when_ready(instance, host_xuid.as_deref(), host_deadline, cancel) {
                    Ok((instance, host)) => (Ok(instance), Some(host)),
                    Err(error) => (Err(error), None),
                }
            }
            started => (started, None),
        };
        let mut life = lock(&self.inner.life);
        if cancel.is_cancelled() {
            drop(host_guard);
            drop(life);
            if let Ok(instance) = started {
                self.stop_instance(instance.as_ref());
            }
            self.idle_if_current(generation);
            return;
        }
        let instance = match started {
            Ok(instance) => instance,
            Err(error) => {
                tracing::error!("local world {} failed to start: {error}", spec.world.id);
                life.failure = Some(error.status_text());
                self.set_state(&mut life, WorldState::Failed);
                return;
            }
        };
        if let Some(world) = fresh_world {
            life.world = Some(world);
        }
        life.instance = Some(Arc::clone(&instance));
        let paused = life.paused && instance.can_pause();
        life.paused = paused;
        self.set_state(&mut life, WorldState::Running);
        drop(host_guard);
        drop(life);
        if let Err(error) = self.inner.store.touch(&spec.world.id, &applied) {
            tracing::warn!("record when {} was last played: {error}", spec.world.id);
        }
        if paused {
            let _ = instance.set_paused(true);
        }
        let watched = Arc::clone(&instance);
        if !self.background(move |manager| manager.watch(&watched)) {
            tracing::warn!(
                "cannot watch local world {} for an unexpected exit",
                spec.world.id
            );
        }
    }

    /// Fails the running world while retaining its backend cleanup barrier for any later open.
    fn watch(&self, instance: &Arc<dyn Instance>) {
        let _ = instance.exited().recv();
        let mut life = lock(&self.inner.life);
        let current = life
            .instance
            .as_ref()
            .is_some_and(|running| Arc::ptr_eq(running, instance));
        if current && life.state == WorldState::Running {
            tracing::error!("the local world server exited unexpectedly");
            let (cleaning, done) = crossbeam_channel::bounded::<()>(0);
            if let Some(world) = &life.world {
                let retiring = Retiring {
                    world_id: world.id.clone(),
                    done,
                };
                life.retiring.push(retiring);
            }
            life.instance = None;
            life.live_applied = None;
            life.failure = Some("local world server exited unexpectedly".to_owned());
            self.set_state(&mut life, WorldState::Failed);
            drop(life);
            // stop joins the backend's shared cleanup even when its process has already exited.
            self.stop_instance(instance.as_ref());
            drop(cleaning);
        }
    }

    /// Stops the server; true once it exited and completed cleanup in time.
    fn stop_instance(&self, instance: &dyn Instance) -> bool {
        let stopped = instance.stop(self.inner.stop_timeout);
        if let Err(error) = &stopped {
            tracing::warn!("stop local world server: {error}");
        }
        stopped.is_ok()
    }

    /// Stops the server, then drops the pending edits it applied live once its successful exit
    /// shows it saved them; otherwise the next start applies them again.
    fn stop_and_acknowledge(
        &self,
        instance: &dyn Instance,
        saved: Option<(String, OptionChanges)>,
    ) {
        if self.stop_instance(instance)
            && instance.exited_cleanly()
            && let Some((id, applied)) = saved.filter(|(_, applied)| !applied.is_empty())
            && let Err(error) = self.inner.store.acknowledge(&id, &applied)
        {
            tracing::warn!("local world {id}: record its saved settings: {error}");
        }
    }

    /// Stops the open world (saving it) or clears a failure, normally without waiting.
    /// If a stop worker cannot start, completes cleanup inline before releasing its barrier.
    pub fn close(&self) {
        let mut life = lock(&self.inner.life);
        let state = life.state;
        match state {
            WorldState::Failed => self.idle(&mut life),
            WorldState::Starting => {
                life.cancel_start = None;
                let start_done = life.start_done.take();
                if let Some((world, done)) = life.world.as_ref().zip(start_done) {
                    let retiring = Retiring {
                        world_id: world.id.clone(),
                        done,
                    };
                    life.retiring.push(retiring);
                }
                self.set_state(&mut life, WorldState::Stopping);
            }
            WorldState::Running => {
                let Some(instance) = life.instance.take() else {
                    self.idle(&mut life);
                    return;
                };
                instance.request_stop();
                // Done only after the stop's cleanup, which may remove a container or a world link
                // by name after the process has exited.
                let (stopping_done, done) = crossbeam_channel::bounded::<()>(0);
                if let Some(world) = &life.world {
                    let retiring = Retiring {
                        world_id: world.id.clone(),
                        done,
                    };
                    life.retiring.push(retiring);
                }
                self.set_state(&mut life, WorldState::Stopping);
                let stopping = Arc::clone(&instance);
                let generation = life.generation;
                let saved = life
                    .live_applied
                    .take()
                    .filter(|(applied_generation, _)| *applied_generation == generation)
                    .zip(life.world.as_ref())
                    .map(|((_, applied), world)| (world.id.clone(), applied));
                let fallback = (stopping_done.clone(), saved.clone());
                let spawned = self.background(move |manager| {
                    manager.stop_and_acknowledge(stopping.as_ref(), saved);
                    drop(stopping_done);
                    manager.idle_if_current(generation);
                });
                if !spawned {
                    drop(life);
                    self.stop_and_acknowledge(instance.as_ref(), fallback.1);
                    drop(fallback.0);
                    self.idle_if_current(generation);
                }
            }
            WorldState::Idle | WorldState::Stopping => {}
        }
    }

    /// Ends any standby server, cancels a start in progress and asks the running server to save
    /// and stop without waiting; the exit sweep then waits out its save budget. A lifecycle lock
    /// held by the exiting thread skips the request.
    pub(crate) fn request_exit_stop(&self) {
        if let Some(runner) = &self.inner.runners.dragonfly {
            runner.discard_standby();
        }
        let mut life = match self.inner.life.try_lock() {
            Ok(life) => life,
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(TryLockError::WouldBlock) => return,
        };
        // A start in progress stops spawning; the sweep ends what it already started.
        life.cancel_start = None;
        let instance = life.instance.clone();
        drop(life);
        if let Some(instance) = instance {
            instance.request_stop();
        }
    }

    /// Closes the open world and waits until its server has exited.
    #[cfg(test)]
    pub(crate) fn shutdown(&self) {
        self.close();
        let running = lock(&self.inner.background);
        drop(
            self.inner
                .background_done
                .wait_while(running, |running| *running > 0)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }

    /// Freezes or resumes the running world; a request while starting applies once it runs.
    pub fn set_paused(&self, paused: bool) -> Result<(), WorldError> {
        let mut life = lock(&self.inner.life);
        let state = life.state;
        match state {
            WorldState::Starting => {
                life.paused = paused;
                Ok(())
            }
            WorldState::Running => {
                let Some(instance) = life.instance.clone() else {
                    return Err(WorldError::NotOpen);
                };
                if !instance.can_pause() {
                    return Ok(());
                }
                life.paused = paused;
                drop(life);
                instance
                    .set_paused(paused)
                    .map_err(|error| WorldError::Failed(error.to_string()))
            }
            WorldState::Idle | WorldState::Stopping | WorldState::Failed => {
                Err(WorldError::NotOpen)
            }
        }
    }

    /// The open world as the core routes and hosts it.
    pub(crate) fn core_report(&self) -> LocalWorld {
        let life = lock(&self.inner.life);
        let target = match (&life.instance, &life.world) {
            (Some(instance), Some(world)) if life.state == WorldState::Running => {
                Some(target_for(world, instance.as_ref()))
            }
            _ => None,
        };
        LocalWorld {
            state: life.state,
            world: life.world.as_ref().map(|world| LocalWorldInfo {
                id: world.id.clone(),
                name: world.name.clone(),
                game_mode: world.game_mode,
                backend: world.backend,
                multiplayer_disabled: !world
                    .options
                    .enabled(launcher::local_worlds::options::WorldOption::Multiplayer),
            }),
            target,
            error: life.failure.clone(),
        }
    }

    /// A receiver that wakes after lifecycle changes and updates to the active world report.
    pub fn subscribe(&self) -> Receiver<()> {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        lock(&self.inner.subscribers).push(sender);
        receiver
    }

    fn set_state(&self, life: &mut Life, state: WorldState) {
        life.state = state;
        self.notify_subscribers();
    }

    /// Wakes report readers, coalescing changes until each reader consumes its notification.
    fn notify_subscribers(&self) {
        lock(&self.inner.subscribers).retain(|subscriber| {
            !matches!(subscriber.try_send(()), Err(TrySendError::Disconnected(())))
        });
    }

    /// Clears the lifecycle, keeping the servers that are still stopping.
    fn idle(&self, life: &mut Life) {
        let mut retiring = std::mem::take(&mut life.retiring);
        retiring.retain(|retiring| !retiring.done());
        *life = Life {
            generation: life.generation,
            retiring,
            ..Life::default()
        };
        self.set_state(life, WorldState::Idle);
    }

    /// Idles the lifecycle unless a later open has taken it over.
    fn idle_if_current(&self, generation: u64) {
        let mut life = lock(&self.inner.life);
        if life.generation == generation {
            self.idle(&mut life);
        }
    }

    /// Runs `task` on its own thread, counted for [`Self::shutdown`]; false when no thread started.
    fn background(&self, task: impl FnOnce(&Self) + Send + 'static) -> bool {
        *lock(&self.inner.background) += 1;
        let manager = self.clone();
        let spawned = thread::Builder::new()
            .name("local-world".to_owned())
            .spawn(move || {
                task(&manager);
                manager.finish_background();
            });
        if let Err(error) = spawned {
            tracing::error!("cannot start a local world thread: {error}");
            self.finish_background();
            return false;
        }
        true
    }

    fn finish_background(&self) {
        let mut running = lock(&self.inner.background);
        *running -= 1;
        if *running == 0 {
            self.inner.background_done.notify_all();
        }
    }
}

#[path = "manager_host.rs"]
mod host;

/// Where the core dials the running world: RakNet for Dragonfly, NetherNet for BDS, through its
/// LAN advertisement when the server publishes one.
fn target_for(world: &World, instance: &dyn Instance) -> LocalTarget {
    let address = instance.address().to_owned();
    match (world.backend, instance.lan_address()) {
        (Backend::Bds, Some(lan)) if !lan.is_empty() => LocalTarget {
            address,
            transport: Transport::NetherNetLan,
            lan_address: Some(lan.to_owned()),
            level_name: Some(world.id.clone()),
        },
        (Backend::Bds, _) => LocalTarget {
            address,
            transport: Transport::NetherNet,
            lan_address: None,
            level_name: None,
        },
        (Backend::Dragonfly, _) => LocalTarget {
            address,
            transport: Transport::RakNet,
            lan_address: None,
            level_name: None,
        },
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
