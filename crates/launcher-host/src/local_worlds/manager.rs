use std::{
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

use bridge::{Backend, LocalTarget, LocalWorld, LocalWorldInfo, Transport, WorldState};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use launcher::local_worlds::world::{
    NewWorld, Prefs, PrefsUpdate, SetupState, World, WorldStatus, WorldUpdate,
};

use super::{
    Cancel, CancelHandle, cancel_pair, lock,
    process::{Instance, Runner, StartSpec},
    provision::{Provisioner, RuntimeInfo, RuntimeKind},
    store::Store,
    world::{WorldError, validate_name},
};

/// BDS saves the world before exiting.
const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(30);
/// Outlasts the Docker probe's own timeout.
const DEFAULT_RUNTIME_WAIT: Duration = Duration::from_secs(25);

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
        }
    }
}

/// BDS when it can run natively or in a container, else Dragonfly.
pub(crate) fn default_backend(runtime: &RuntimeInfo) -> Backend {
    match runtime.kind {
        RuntimeKind::Native | RuntimeKind::Container => Backend::Bds,
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
    subscribers: Mutex<Vec<Sender<()>>>,
    /// Background starts, stops and exit watches still running.
    background: Mutex<usize>,
    background_done: Condvar,
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
            inner: Arc::new(Inner {
                store: parts.store,
                runners: parts.runners,
                setup: parts.setup,
                unavailable: parts.unavailable,
                stop_timeout: parts.stop_timeout,
                runtime_wait: parts.runtime_wait,
                life: Mutex::default(),
                subscribers: Mutex::default(),
                background: Mutex::new(0),
                background_done: Condvar::new(),
            }),
        }
    }

    /// Saved worlds, most recently played first.
    pub fn list(&self) -> Result<Vec<World>, WorldError> {
        self.inner.store.list()
    }

    /// Saves a new world; a BDS world is refused where BDS cannot run. Unless Dragonfly was asked
    /// for, it first waits out a runtime detection in flight, so no world is saved against a guess.
    pub fn create(&self, world: &NewWorld) -> Result<World, WorldError> {
        let mut world = world.clone();
        world.name = validate_name(&world.name)?;
        if let Some(setup) = &self.inner.setup
            && world.backend != Some(Backend::Dragonfly)
            && !setup.await_runtime(self.inner.runtime_wait)
        {
            return Err(WorldError::RuntimePending);
        }
        let backend = world
            .backend
            .unwrap_or_else(|| self.inner.store.default_backend());
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
        world.backend = Some(backend);
        self.inner.store.create(&world)
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

    /// Deletes a world that is not starting, running or stopping.
    pub fn delete(&self, id: &str) -> Result<(), WorldError> {
        let life = lock(&self.inner.life);
        let open = !matches!(life.state, WorldState::Idle | WorldState::Failed);
        if open && life.world.as_ref().is_some_and(|world| world.id == id) {
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

    /// Begins starting a world and returns at once; poll [`Self::status`] for readiness.
    /// Reopening the world that is already starting or running does nothing.
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
            WorldState::Stopping => return Err(WorldError::Busy),
            WorldState::Idle | WorldState::Failed => {}
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
        life.world = Some(world.clone());
        life.instance = None;
        life.failure = None;
        life.paused = false;
        life.cancel_start = Some(handle);
        self.set_state(&mut life, WorldState::Starting);
        let spec = StartSpec {
            world,
            dir,
            view_distance,
        };
        if !self.background(move |manager| manager.start(&spec, &cancel)) {
            life.failure = Some(WorldError::Cancelled.status_text());
            self.set_state(&mut life, WorldState::Failed);
        }
        Ok(())
    }

    fn start(&self, spec: &StartSpec, cancel: &Cancel) {
        let started = self.inner.runners.start(spec, cancel);
        let mut life = lock(&self.inner.life);
        if cancel.is_cancelled() {
            drop(life);
            if let Ok(instance) = started {
                self.stop_instance(instance.as_ref());
            }
            self.idle(&mut lock(&self.inner.life));
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
        life.instance = Some(Arc::clone(&instance));
        let paused = life.paused && instance.can_pause();
        life.paused = paused;
        self.set_state(&mut life, WorldState::Running);
        drop(life);
        if let Err(error) = self.inner.store.touch(&spec.world.id) {
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

    /// Fails the world when its server exits while it is still the running one.
    fn watch(&self, instance: &Arc<dyn Instance>) {
        let _ = instance.exited().recv();
        let mut life = lock(&self.inner.life);
        let current = life
            .instance
            .as_ref()
            .is_some_and(|running| Arc::ptr_eq(running, instance));
        if current && life.state == WorldState::Running {
            tracing::error!("the local world server exited unexpectedly");
            life.instance = None;
            life.failure = Some("local world server exited unexpectedly".to_owned());
            self.set_state(&mut life, WorldState::Failed);
        }
    }

    fn stop_instance(&self, instance: &dyn Instance) {
        if let Err(error) = instance.stop(self.inner.stop_timeout) {
            tracing::warn!("stop local world server: {error}");
        }
    }

    /// Stops the open world (saving it) or clears a failure; it returns without waiting for the
    /// server, which has already been asked to stop.
    pub fn close(&self) {
        let mut life = lock(&self.inner.life);
        let state = life.state;
        match state {
            WorldState::Failed => self.idle(&mut life),
            WorldState::Starting => {
                life.cancel_start = None;
                self.set_state(&mut life, WorldState::Stopping);
            }
            WorldState::Running => {
                let Some(instance) = life.instance.take() else {
                    self.idle(&mut life);
                    return;
                };
                instance.request_stop();
                self.set_state(&mut life, WorldState::Stopping);
                let stopping = Arc::clone(&instance);
                let spawned = self.background(move |manager| {
                    manager.stop_instance(stopping.as_ref());
                    manager.idle(&mut lock(&manager.inner.life));
                });
                if !spawned {
                    self.idle(&mut life);
                }
            }
            WorldState::Idle | WorldState::Stopping => {}
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
            }),
            target,
            error: life.failure.clone(),
        }
    }

    /// A receiver that wakes after each lifecycle change.
    pub(crate) fn subscribe(&self) -> Receiver<()> {
        let (sender, receiver) = crossbeam_channel::bounded(1);
        lock(&self.inner.subscribers).push(sender);
        receiver
    }

    fn set_state(&self, life: &mut Life, state: WorldState) {
        life.state = state;
        lock(&self.inner.subscribers).retain(|subscriber| {
            !matches!(subscriber.try_send(()), Err(TrySendError::Disconnected(())))
        });
    }

    fn idle(&self, life: &mut Life) {
        *life = Life::default();
        self.set_state(life, WorldState::Idle);
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
