use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
#[cfg(any(feature = "native-networking", test))]
use std::{collections::HashSet, io, path::PathBuf, thread, time::Duration};

#[cfg(any(feature = "native-networking", test))]
use crossbeam_channel::unbounded;
use crossbeam_channel::{Receiver, Sender};
#[cfg(any(feature = "native-networking", test))]
use launcher::local_worlds::world::PrefsUpdate;
use launcher::local_worlds::world::WorldThumbnail;
#[cfg(any(feature = "native-networking", test))]
use launcher_host::desktop::picker::{self, Filter};
use launcher_host::local_worlds::Manager;
#[cfg(any(feature = "native-networking", test))]
use launcher_host::local_worlds::{WORLD_ARCHIVE_EXTENSION, WorldError};

use super::model::{Effect, Event};

/// Spaces preference polls, and bounds a status poll that no world change answers sooner.
#[cfg(any(feature = "native-networking", test))]
const POLL_DELAY: Duration = Duration::from_millis(250);

#[cfg(any(feature = "native-networking", test))]
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
    SaveThumbnail {
        id: String,
        image: Arc<[u8]>,
    },
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    LeaveNative(launcher_host::local_worlds::NativeLocalWorldSelection),
}

/// Enqueues retirement of one original opening when its session owner ends.
/// Dropping this capability never writes the process pipe on the caller's thread.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub struct NativeLocalWorldRetirement {
    requests: Sender<Request>,
    selection: Option<launcher_host::local_worlds::NativeLocalWorldSelection>,
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
impl Drop for NativeLocalWorldRetirement {
    fn drop(&mut self) {
        if let Some(selection) = self.selection.take() {
            let _ = self.requests.send(Request::LeaveNative(selection));
        }
    }
}

/// A menu answer and newly loaded thumbnails, read together by the world worker.
pub(super) struct Reply {
    pub(super) event: Event,
    pub(super) thumbnails: Vec<(String, WorldThumbnail)>,
}

/// Reads uncached thumbnails on the worker and forgets deleted worlds.
#[cfg(any(feature = "native-networking", test))]
fn read_thumbnails(
    manager: &Manager,
    event: &Event,
    loaded: &mut HashSet<String>,
) -> Vec<(String, WorldThumbnail)> {
    let worlds = match event {
        Event::Listed(worlds) => worlds.as_slice(),
        Event::Created(world) | Event::Updated(world) => std::slice::from_ref(world),
        Event::Applied { applied, .. } => std::slice::from_ref(&applied.world),
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
/// original manager's process lifetime current.
pub struct WorldsClient {
    effects: Sender<Request>,
    events: Receiver<Reply>,
    /// Set once the client is detached; an open in progress finishes before it is set.
    revoked: Arc<Mutex<bool>>,
    #[cfg(test)]
    worker: Option<thread::JoinHandle<()>>,
}

impl WorldsClient {
    /// Starts the original world worker; it exits when its final request owner is dropped.
    #[cfg(any(feature = "native-networking", test))]
    pub fn spawn(manager: Manager) -> io::Result<Self> {
        Self::spawn_polling(manager, POLL_DELAY)
    }

    #[cfg(any(feature = "native-networking", test))]
    fn spawn_polling(manager: Manager, poll_delay: Duration) -> io::Result<Self> {
        let changes = manager.subscribe();
        let revoked = Arc::new(Mutex::new(false));
        let worker_revoked = Arc::clone(&revoked);
        let (effects, effect_rx) = unbounded::<Request>();
        let (event_tx, events) = unbounded::<Reply>();
        let _worker = thread::Builder::new()
            .name("local-worlds".to_owned())
            .spawn(move || {
                manager.reclaim_abandoned_imports();
                let mut loaded = HashSet::new();
                while let Ok(request) = effect_rx.recv() {
                    let effect = match request {
                        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                        Request::LeaveNative(selection) => {
                            selection.request_leave();
                            continue;
                        }
                        Request::Effect(effect) => effect,
                        Request::SaveThumbnail { id, image } => {
                            if let Err(error) = manager.save_thumbnail(&id, &image) {
                                bevy::log::warn!(%error, "could not save the world thumbnail");
                            }
                            continue;
                        }
                    };
                    match effect {
                        // A world change answers at once.
                        Effect::PollStatus => crossbeam_channel::select! {
                            recv(changes) -> _ => {}
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
                        // An active native session may still own original-opening retirement.
                        // Its final sender keeps only this existing worker alive for that request.
                        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                        continue;
                        #[cfg(not(all(
                            feature = "native-networking",
                            not(target_arch = "wasm32")
                        )))]
                        break;
                    }
                    // Only an open holds the gate, so a revoked client never starts a world
                    // while slow requests (a Docker probe) never delay a detach.
                    let gate = matches!(effect, Effect::Open(_) | Effect::Prestart).then_some(gate);
                    let cancelled = || *lock(&worker_revoked);
                    let event = execute(&manager, effect, &cancelled);
                    drop(gate);
                    let event = event.map(|event| match event {
                        Event::Failed(message) if preferences => Event::FailedPrefs(message),
                        event => event,
                    });
                    if let Some(event) = event {
                        let thumbnails = read_thumbnails(&manager, &event, &mut loaded);
                        if event_tx.send(Reply { event, thumbnails }).is_err() {
                            // Retained native retirement senders still own this request receiver.
                            // Seal ordinary effects while the same queue finishes their cleanup.
                            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                            {
                                *lock(&worker_revoked) = true;
                                continue;
                            }
                            #[cfg(not(all(
                                feature = "native-networking",
                                not(target_arch = "wasm32")
                            )))]
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            effects,
            events,
            revoked,
            #[cfg(test)]
            worker: Some(_worker),
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

    /// Queues original-opening cleanup on the same process/effect worker.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(super) fn leave_native(
        &self,
        selection: launcher_host::local_worlds::NativeLocalWorldSelection,
    ) {
        let _ = self.effects.send(Request::LeaveNative(selection));
    }

    /// Keeps the same request receiver available until the session releases its original opening.
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    pub(super) fn native_retirement(
        &self,
        selection: launcher_host::local_worlds::NativeLocalWorldSelection,
    ) -> NativeLocalWorldRetirement {
        NativeLocalWorldRetirement {
            requests: self.effects.clone(),
            selection: Some(selection),
        }
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

#[cfg(any(feature = "native-networking", test))]
fn failed(error: WorldError) -> Event {
    Event::Failed(error.public_message())
}

#[cfg(any(feature = "native-networking", test))]
fn prefs_event(manager: &Manager, update: PrefsUpdate) -> Event {
    manager
        .prefs(update)
        .map_or_else(failed, |prefs| Event::Prefs(prefs, manager.status()))
}

/// Runs one effect against the same original manager.
/// `cancelled` reports a detach, which an import in progress obeys by publishing nothing.
#[cfg(any(feature = "native-networking", test))]
fn execute(
    manager: &Manager,
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
        Effect::Apply {
            id,
            revision,
            update,
        } => match manager.apply(&id, &update) {
            Ok(applied) => Event::Applied { revision, applied },
            Err(error) => Event::ApplyFailed {
                id,
                revision,
                error: error.public_message(),
            },
        },
        Effect::Open(id) => match manager.open(&id, None) {
            Ok(()) => Event::Status(manager.status()),
            Err(WorldError::EulaRequired) => Event::EulaRequired,
            Err(error) => failed(error),
        },
        Effect::LoadPrefs | Effect::PollPrefs => prefs_event(manager, PrefsUpdate::default()),
        Effect::SetPrefs {
            dismiss_docker_prompt,
            redetect,
        } => prefs_event(
            manager,
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
        Effect::PollStatus => Event::Status(manager.status()),
        Effect::Close => {
            manager.close();
            return None;
        }
        Effect::Leave => {
            manager.leave();
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
        Effect::Prestart => {
            manager.prestart();
            return None;
        }
        Effect::OpenUrl(_) => return None,
    })
}

/// Imports each file in turn; a refusal names its file.
#[cfg(any(feature = "native-networking", test))]
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
    use launcher::local_worlds::world::{Generator, NewWorld};

    /// A metadata write failure restores the saved selection without leaving world settings.
    #[test]
    fn a_failed_metadata_save_rolls_back_and_allows_retry() {
        use launcher::local_worlds::{
            in_world::Action,
            model::{Input, Screen, WorldsMenu},
            world::{Difficulty, Generator, NewWorld},
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("worlds");
        let manager = launcher_host::local_worlds::open_store(root.clone()).unwrap();
        let world = manager
            .create(&NewWorld {
                name: "Home".into(),
                generator: Generator::Flat,
                ..Default::default()
            })
            .unwrap();
        // Loading existing BDS metadata needs no installed server or running backend.
        let metadata = root.join(&world.id).join("world.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&metadata).unwrap()).unwrap();
        record["backend"] = serde_json::json!(bridge::Backend::Bds);
        std::fs::write(&metadata, serde_json::to_vec(&record).unwrap()).unwrap();
        let world = manager.list().unwrap().pop().unwrap();
        let mut menu = WorldsMenu::default();
        menu.apply(Event::Listed(vec![world.clone()]));
        let input = Input::InWorld(world.id.clone(), Action::Difficulty(Difficulty::Hard));
        let effect = menu.update(input.clone()).pop().unwrap();
        std::fs::rename(&root, dir.path().join("saved-worlds")).unwrap();
        std::fs::write(&root, b"blocks the store directory").unwrap();
        let event = execute(&manager, effect, &|| false).unwrap();
        assert!(matches!(&event, Event::ApplyFailed { id, .. } if id == &world.id));
        menu.apply(event);
        assert_eq!(
            menu.screen(),
            Screen::List,
            "the general error screen is not used"
        );
        assert_eq!(menu.worlds()[0].difficulty, world.difficulty);
        assert!(menu.in_world_view(&world.id).unwrap().error.is_some());
        let mut retry = menu.update(input);
        assert_eq!(retry.len(), 1, "the same choice can retry");
        std::fs::remove_file(&root).unwrap();
        std::fs::rename(dir.path().join("saved-worlds"), &root).unwrap();
        menu.apply(execute(&manager, retry.pop().unwrap(), &|| false).unwrap());
        assert_eq!(menu.worlds()[0].difficulty, Difficulty::Hard);
        assert!(menu.in_world_view(&world.id).unwrap().error.is_none());
    }

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
        let worlds = tempfile::tempdir().expect("worlds folder");
        let manager =
            launcher_host::local_worlds::open_store(worlds.path().to_path_buf()).expect("manager");
        let world = manager
            .create(&NewWorld {
                name: "w".into(),
                generator: Generator::Flat,
                backend: Some(bridge::Backend::Dragonfly),
                ..NewWorld::default()
            })
            .expect("create");
        let client = WorldsClient::spawn(manager.clone()).expect("spawn worker");
        client.revoke();
        client.send(Effect::Open(world.id));
        let mut client = client;
        let worker = client.worker.take().unwrap();
        drop(client);
        worker.join().unwrap();
        assert_eq!(manager.status().state, bridge::WorldState::Idle);
    }

    /// A status poll answers when the world changes instead of waiting out its delay.
    #[test]
    fn a_status_poll_answers_as_soon_as_the_world_changes() {
        let worlds = tempfile::tempdir().expect("worlds folder");
        let manager =
            launcher_host::local_worlds::open_store(worlds.path().to_path_buf()).expect("manager");
        let world = manager
            .create(&NewWorld {
                name: "w".into(),
                generator: Generator::Flat,
                backend: Some(bridge::Backend::Dragonfly),
                ..NewWorld::default()
            })
            .expect("create");
        let client = WorldsClient::spawn_polling(manager.clone(), Duration::from_secs(3600))
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

    /// Builds a minimal flat-world level.dat for the import worker.
    fn flat_level_dat() -> Vec<u8> {
        let nbt = b"\x0a\0\0\x03\x09\0Generator\x02\0\0\0\0";
        let mut level = 10u32.to_le_bytes().to_vec();
        level.extend(
            u32::try_from(nbt.len())
                .expect("fixture length")
                .to_le_bytes(),
        );
        level.extend(nbt);
        level
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
        zip.write_all(&flat_level_dat()).expect("body");
        let good = worlds.path().join("Home.mcworld");
        std::fs::write(&good, zip.finish().expect("zip").into_inner()).expect("archive");
        let bad = worlds.path().join("Broken.mcworld");
        std::fs::write(&bad, b"not a zip").expect("archive");
        let client = WorldsClient::spawn(manager).expect("spawn worker");
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
        assert_eq!(worlds[0].generator, Generator::Flat);
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
        let client = WorldsClient::spawn(manager).expect("spawn worker");
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

#[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
mod native_retirement_tests;
