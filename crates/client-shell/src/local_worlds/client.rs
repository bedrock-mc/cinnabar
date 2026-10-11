use std::{
    io,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use launcher::local_worlds::world::PrefsUpdate;
use launcher_host::local_worlds::{CoreLink, CoreRoute, Manager, WorldError};

use super::model::{Effect, Event};

const POLL_DELAY: Duration = Duration::from_millis(250);

/// Runs [`Effect`]s against the world manager on a worker thread, in order, and keeps the
/// attached core's route to the open world current.
pub struct WorldsClient {
    effects: Sender<Effect>,
    events: Receiver<Event>,
    /// Set once the client is detached; an open in progress finishes before it is set.
    revoked: Arc<Mutex<bool>>,
    _core: CoreLink,
}

impl WorldsClient {
    /// Starts the worker; it exits when the client is dropped.
    pub fn spawn(manager: Manager, socket_dir: PathBuf) -> io::Result<Self> {
        let core = CoreLink::spawn(manager.clone(), socket_dir)?;
        let route = core.route();
        let revoked = Arc::new(Mutex::new(false));
        let worker_revoked = Arc::clone(&revoked);
        let (effects, effect_rx) = unbounded::<Effect>();
        let (event_tx, events) = unbounded::<Event>();
        thread::Builder::new()
            .name("local-worlds".to_owned())
            .spawn(move || {
                while let Ok(effect) = effect_rx.recv() {
                    if matches!(effect, Effect::PollStatus | Effect::PollPrefs) {
                        thread::sleep(POLL_DELAY);
                    }
                    let preferences = matches!(
                        effect,
                        Effect::LoadPrefs | Effect::PollPrefs | Effect::SetPrefs { .. }
                    );
                    let gate = lock(&worker_revoked);
                    if *gate {
                        break;
                    }
                    // Only an open holds the gate, so a revoked client never starts a world
                    // while slow requests (a Docker probe) never delay a detach.
                    let gate = matches!(effect, Effect::Open(_)).then_some(gate);
                    let event = execute(&manager, &route, effect);
                    drop(gate);
                    let event = event.map(|event| match event {
                        Event::Failed(message) if preferences => Event::FailedPrefs(message),
                        event => event,
                    });
                    if let Some(event) = event
                        && event_tx.send(event).is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            effects,
            events,
            revoked,
            _core: core,
        })
    }

    /// Stops the worker from starting anything else, waiting out an open in progress, so a
    /// world closed after this stays closed.
    pub fn revoke(&self) {
        *lock(&self.revoked) = true;
    }

    pub fn send(&self, effect: Effect) {
        let _ = self.effects.send(effect);
    }

    pub fn drain(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn failed(error: WorldError) -> Event {
    Event::Failed(error.public_message())
}

fn prefs_event(manager: &Manager, route: &CoreRoute, update: PrefsUpdate) -> Event {
    manager.prefs(update).map_or_else(failed, |prefs| {
        Event::Prefs(prefs, route.gate(manager.status()))
    })
}

/// Runs one effect; reported statuses hold a running world at starting until the core routes it.
fn execute(manager: &Manager, route: &CoreRoute, effect: Effect) -> Option<Event> {
    Some(match effect {
        Effect::List => manager.list().map_or_else(failed, Event::Listed),
        Effect::Create(world) => manager.create(&world).map_or_else(failed, Event::Created),
        Effect::Delete(id) => match manager.delete(&id) {
            Ok(()) => Event::Deleted(id),
            Err(error) => failed(error),
        },
        Effect::Update { id, update } => manager
            .update(&id, &update)
            .map_or_else(failed, Event::Updated),
        Effect::Open(id) => match manager.open(&id, None) {
            Ok(()) => Event::Status(route.gate(manager.status())),
            Err(WorldError::EulaRequired) => Event::EulaRequired,
            Err(error) => failed(error),
        },
        Effect::LoadPrefs | Effect::PollPrefs => {
            prefs_event(manager, route, PrefsUpdate::default())
        }
        Effect::SetPrefs {
            dismiss_docker_prompt,
            redetect,
        } => prefs_event(
            manager,
            route,
            PrefsUpdate {
                docker_prompt_dismissed: dismiss_docker_prompt.then_some(true),
                redetect,
            },
        ),
        Effect::AcceptEula => manager
            .accept_eula()
            .map_or_else(failed, |()| Event::EulaAccepted),
        Effect::PollStatus => Event::Status(route.gate(manager.status())),
        Effect::Close => {
            manager.close();
            return None;
        }
        Effect::SetPaused(paused) => {
            let _ = manager.set_paused(paused);
            return None;
        }
        Effect::OpenUrl(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_show_only_public_messages() {
        assert!(
            matches!(failed(WorldError::NotFound), Event::Failed(message) if message == "world not found")
        );
        let private = WorldError::Failed("open /Users/x/secret: denied".to_owned());
        assert!(matches!(failed(private), Event::Failed(message) if !message.contains("secret")));
    }

    /// A detached client's worker never starts a world after the detach closed the old one.
    #[test]
    fn a_revoked_worker_never_opens_a_world() {
        use launcher::local_worlds::world::NewWorld;
        let worlds = tempfile::tempdir().expect("worlds folder");
        let manager =
            launcher_host::local_worlds::open_store(worlds.path().to_path_buf()).expect("manager");
        let world = manager
            .create(&NewWorld {
                name: "w".into(),
                ..NewWorld::default()
            })
            .expect("create");
        let client = WorldsClient::spawn(manager.clone(), worlds.path().join("no-core-here"))
            .expect("spawn worker");
        client.revoke();
        client.send(Effect::Open(world.id));
        test_time::eventually_within(Duration::from_secs(5), "the revoked worker to stop", || {
            matches!(
                client.events.try_recv(),
                Err(crossbeam_channel::TryRecvError::Disconnected)
            )
        });
        assert_eq!(manager.status().state, bridge::WorldState::Idle);
    }

    #[test]
    fn the_worker_answers_from_the_manager() {
        let worlds = tempfile::tempdir().expect("worlds folder");
        let manager =
            launcher_host::local_worlds::open_store(worlds.path().to_path_buf()).expect("manager");
        let core = worlds.path().join("no-core-here");
        let client = WorldsClient::spawn(manager, core).expect("spawn worker");
        client.send(Effect::List);
        let mut events = Vec::new();
        test_time::eventually_within(Duration::from_secs(5), "an event from the worker", || {
            events.extend(client.drain());
            !events.is_empty()
        });
        assert!(matches!(&events[0], Event::Listed(worlds) if worlds.is_empty()));
    }
}
