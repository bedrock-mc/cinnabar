use std::{
    collections::HashSet,
    io,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use launcher::local_worlds::world::{PrefsUpdate, WorldThumbnail};
use launcher_host::desktop::picker::{self, Filter};
use launcher_host::local_worlds::{
    CoreLink, CoreRoute, Manager, WORLD_ARCHIVE_EXTENSION, WorldError,
};

use super::model::{Effect, Event};

/// Spaces preference polls, and bounds a status poll that no world change answers sooner.
const POLL_DELAY: Duration = Duration::from_millis(250);

const WORLD_FILES: Filter = Filter {
    title: "Import world",
    name: "Minecraft worlds",
    extensions: &[WORLD_ARCHIVE_EXTENSION],
    unavailable_hint: "Drop a world file onto the Worlds tab to import it.",
    max_bytes: None,
};

/// Work kept on the local-world worker, including thumbnail persistence.
enum Request {
    Effect(Effect),
    SaveThumbnail { id: String, image: Arc<[u8]> },
}

/// A menu answer and newly loaded thumbnails, read together by the world worker.
pub(super) struct Reply {
    pub(super) event: Event,
    pub(super) thumbnails: Vec<(String, WorldThumbnail)>,
}

/// Reads uncached thumbnails on the worker and forgets deleted worlds.
fn read_thumbnails(
    manager: &Manager,
    event: &Event,
    loaded: &mut HashSet<String>,
) -> Vec<(String, WorldThumbnail)> {
    let worlds = match event {
        Event::Listed(worlds) => worlds.as_slice(),
        Event::Created(world) | Event::Updated(world) => std::slice::from_ref(world),
        Event::Imported { worlds, .. } => worlds.as_slice(),
        Event::Deleted(id) => {
            loaded.remove(id);
            return Vec::new();
        }
        _ => return Vec::new(),
    };
    worlds
        .iter()
        .filter_map(|world| {
            if loaded.contains(&world.id) {
                return None;
            }
            let image = manager.thumbnail(&world.id)?;
            loaded.insert(world.id.clone());
            Some((
                world.id.clone(),
                WorldThumbnail::new(&world.id, Arc::from(image)),
            ))
        })
        .collect()
}

/// Runs [`Effect`]s against the world manager on a worker thread, in order, and keeps the
/// attached core's route to the open world current.
pub struct WorldsClient {
    effects: Sender<Request>,
    events: Receiver<Reply>,
    /// Set once the client is detached; an open in progress finishes before it is set.
    revoked: Arc<Mutex<bool>>,
    _core: CoreLink,
}

impl WorldsClient {
    /// Starts the worker; it exits when the client is dropped.
    pub fn spawn(manager: Manager, socket_dir: PathBuf) -> io::Result<Self> {
        Self::spawn_polling(manager, socket_dir, POLL_DELAY)
    }

    fn spawn_polling(
        manager: Manager,
        socket_dir: PathBuf,
        poll_delay: Duration,
    ) -> io::Result<Self> {
        let core = CoreLink::spawn(manager.clone(), socket_dir)?;
        let route = core.route();
        let (changes, routed) = (manager.subscribe(), route.changes());
        let revoked = Arc::new(Mutex::new(false));
        let worker_revoked = Arc::clone(&revoked);
        let (effects, effect_rx) = unbounded::<Request>();
        let (event_tx, events) = unbounded::<Reply>();
        thread::Builder::new()
            .name("local-worlds".to_owned())
            .spawn(move || {
                manager.reclaim_abandoned_imports();
                let mut loaded = HashSet::new();
                while let Ok(request) = effect_rx.recv() {
                    let effect = match request {
                        Request::Effect(effect) => effect,
                        Request::SaveThumbnail { id, image } => {
                            if let Err(error) = manager.save_thumbnail(&id, &image) {
                                bevy::log::warn!(%error, "could not save the world thumbnail");
                            }
                            continue;
                        }
                    };
                    match effect {
                        // A world change or the core routing it answers at once.
                        Effect::PollStatus => crossbeam_channel::select! {
                            recv(changes) -> _ => {}
                            recv(routed) -> _ => {}
                            default(poll_delay) => {}
                        },
                        Effect::PollPrefs => thread::sleep(poll_delay),
                        _ => {}
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
                    let cancelled = || *lock(&worker_revoked);
                    let event = execute(&manager, &route, effect, &cancelled);
                    drop(gate);
                    let event = event.map(|event| match event {
                        Event::Failed(message) if preferences => Event::FailedPrefs(message),
                        event => event,
                    });
                    if let Some(event) = event {
                        let thumbnails = read_thumbnails(&manager, &event, &mut loaded);
                        if event_tx.send(Reply { event, thumbnails }).is_err() {
                            break;
                        }
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
        let _ = self.effects.send(Request::Effect(effect));
    }

    /// Queues a write without opening files or acquiring the world store's lock.
    pub(super) fn save_thumbnail(&self, id: String, image: Arc<[u8]>) {
        let _ = self.effects.send(Request::SaveThumbnail { id, image });
    }

    pub(super) fn drain(&self) -> Vec<Reply> {
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
    manager
        .prefs(update)
        .map_or_else(failed, |prefs| Event::Prefs(prefs, route.status(manager)))
}

/// Runs one effect; reported statuses hold a running world at starting until the core routes it.
/// `cancelled` reports a detach, which an import in progress obeys by publishing nothing.
fn execute(
    manager: &Manager,
    route: &CoreRoute,
    effect: Effect,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Event> {
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
            Ok(()) => Event::Status(route.status(manager)),
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
        Effect::Import(Some(files)) => import_files(manager, files, cancelled),
        Effect::Import(None) => match picker::pick(&WORLD_FILES) {
            Ok(file) => import_files(manager, file.into_iter().collect(), cancelled),
            Err(error) => Event::Imported {
                worlds: Vec::new(),
                errors: vec![error],
            },
        },
        Effect::PollStatus => Event::Status(route.status(manager)),
        Effect::Close => {
            manager.close();
            return None;
        }
        Effect::SetPaused(paused) => {
            let _ = manager.set_paused(paused);
            return None;
        }
        Effect::Prepare => {
            manager.prepare();
            return None;
        }
        Effect::OpenUrl(_) => return None,
    })
}

/// Imports each file in turn; a refusal names its file.
fn import_files(
    manager: &Manager,
    files: Vec<PathBuf>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Event {
    let mut worlds = Vec::new();
    let mut errors = Vec::new();
    for file in files {
        match manager.import(&file, cancelled) {
            Ok(world) => worlds.push(world),
            Err(error) => {
                bevy::log::warn!(%error, "could not import a world");
                let name = file
                    .file_name()
                    .unwrap_or(file.as_os_str())
                    .to_string_lossy();
                errors.push(format!("{name}: {}", error.public_message()));
            }
        }
    }
    Event::Imported { worlds, errors }
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

    /// A status poll answers when the world changes instead of waiting out its delay.
    #[test]
    fn a_status_poll_answers_as_soon_as_the_world_changes() {
        use launcher::local_worlds::world::NewWorld;
        let worlds = tempfile::tempdir().expect("worlds folder");
        let manager =
            launcher_host::local_worlds::open_store(worlds.path().to_path_buf()).expect("manager");
        let world = manager
            .create(&NewWorld {
                name: "w".into(),
                backend: Some(bridge::Backend::Dragonfly),
                ..NewWorld::default()
            })
            .expect("create");
        let client = WorldsClient::spawn_polling(
            manager.clone(),
            worlds.path().join("no-core-here"),
            Duration::from_secs(3600),
        )
        .expect("spawn worker");
        client.send(Effect::PollStatus);
        manager.open(&world.id, None).expect("open");
        let mut events = Vec::new();
        test_time::eventually_within(Duration::from_secs(5), "a status after the change", || {
            events.extend(client.drain());
            !events.is_empty()
        });
        assert!(matches!(&events[0].event, Event::Status(_)));
    }

    /// Dropped files import in one answer: each accepted world, and each refusal by file name.
    #[test]
    fn the_worker_imports_every_dropped_file() {
        use std::io::Write;
        let worlds = tempfile::tempdir().expect("worlds folder");
        let manager =
            launcher_host::local_worlds::open_store(worlds.path().join("store")).expect("manager");
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("Home/level.dat", zip::write::SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(b"level").expect("body");
        let good = worlds.path().join("Home.mcworld");
        std::fs::write(&good, zip.finish().expect("zip").into_inner()).expect("archive");
        let bad = worlds.path().join("Broken.mcworld");
        std::fs::write(&bad, b"not a zip").expect("archive");
        let client =
            WorldsClient::spawn(manager, worlds.path().join("no-core-here")).expect("spawn worker");
        client.send(Effect::Import(Some(vec![good, bad])));
        let mut events = Vec::new();
        test_time::eventually_within(Duration::from_secs(5), "the import result", || {
            events.extend(client.drain());
            !events.is_empty()
        });
        let Event::Imported { worlds, errors } = &events[0].event else {
            panic!("{:?}", events[0].event);
        };
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].name, "Home");
        assert!(
            errors.len() == 1 && errors[0].starts_with("Broken.mcworld: "),
            "{errors:?}"
        );
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
        assert!(matches!(&events[0].event, Event::Listed(worlds) if worlds.is_empty()));
    }
}

#[cfg(test)]
mod thumbnail_tests;
