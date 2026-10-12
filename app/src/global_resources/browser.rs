//! A browser has no pack folder and no worker thread. The page keeps imported packs in its
//! IndexedDB store, `cinnabarPackStore` (web/site/boot.js), and this driver runs the same pack
//! session as a desktop's worker between frames, loading an archive into memory only while the
//! session reads it.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use crossbeam_channel::{Receiver, Sender};
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use resource_pack::{AdmissionError, LibraryError, PackStorage};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::worker::{Command, Event, Session};

/// The page's store of imported packs, by file name.
const PAGE_STORE: &str = "cinnabarPackStore";

/// Pack storage the page persists; holds in memory the catalog, icons, and any archive loaded
/// for the command being handled.
#[derive(Clone)]
struct BrowserStorage {
    root: PathBuf,
    files: Arc<Mutex<Files>>,
}

#[derive(Default)]
struct Files {
    resident: HashMap<String, Arc<[u8]>>,
    stored: HashSet<String>,
}

/// Whether `name` is a pack archive, which stays in the page until a command reads it.
fn archive(name: &str) -> bool {
    resource_pack::is_pack_import_path(Path::new(name))
}

impl BrowserStorage {
    fn files(&self) -> std::sync::MutexGuard<'_, Files> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes `name`'s bytes readable, and its icon visible to the menus.
    fn keep(&self, name: &str, bytes: Arc<[u8]>) {
        if !archive(name) {
            assets::carriers::file::add_downloaded(&self.path(name), Arc::clone(&bytes));
        }
        self.files().resident.insert(name.to_owned(), bytes);
    }

    /// Loads each of `names` the page stores and memory lacks.
    async fn load(&self, names: impl IntoIterator<Item = String>) {
        for name in names {
            let missing = {
                let files = self.files();
                files.stored.contains(&name) && !files.resident.contains_key(&name)
            };
            if !missing {
                continue;
            }
            match call("read", &[JsValue::from_str(&name)]).await {
                Some(value) if value.is_instance_of::<Uint8Array>() => {
                    self.keep(&name, Uint8Array::from(value).to_vec().into());
                }
                _ => bevy::log::warn!(name, "imported resource pack file is unreadable"),
            }
        }
    }

    /// Drops loaded archives once the command that read them is done.
    fn release_archives(&self) {
        self.files().resident.retain(|name, _| !archive(name));
    }
}

impl PackStorage for BrowserStorage {
    fn read(&self, name: &str, limit: usize) -> Result<Vec<u8>, LibraryError> {
        match self.files().resident.get(name) {
            Some(bytes) if bytes.len() > limit => Err(AdmissionError::ArchiveTooLarge.into()),
            Some(bytes) => Ok(bytes.to_vec()),
            None => Err(std::io::Error::from(std::io::ErrorKind::NotFound).into()),
        }
    }

    fn contains(&self, name: &str) -> bool {
        let files = self.files();
        files.stored.contains(name) || files.resident.contains_key(name)
    }

    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), LibraryError> {
        self.keep(name, bytes.into());
        self.files().stored.insert(name.to_owned());
        let (name, bytes) = (name.to_owned(), Uint8Array::from(bytes));
        wasm_bindgen_futures::spawn_local(async move {
            if call("write", &[JsValue::from_str(&name), bytes.into()])
                .await
                .is_none()
            {
                bevy::log::warn!(name, "could not keep an imported resource pack file");
            }
        });
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<(), LibraryError> {
        let removed = {
            let mut files = self.files();
            let resident = files.resident.remove(name).is_some();
            files.stored.remove(name) || resident
        };
        if !removed {
            return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
        }
        let name = name.to_owned();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = call("remove", &[JsValue::from_str(&name)]).await;
        });
        Ok(())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

/// Calls the page store's `method`, resolving its promise; `None` when the page has no store or
/// the call fails.
async fn call(method: &str, arguments: &[JsValue]) -> Option<JsValue> {
    let store = Reflect::get(&js_sys::global(), &PAGE_STORE.into())
        .ok()
        .filter(JsValue::is_object)?;
    let function: Function = Reflect::get(&store, &method.into()).ok()?.dyn_into().ok()?;
    let arguments: Array = arguments.iter().collect();
    let promise: Promise = function.apply(&store, &arguments).ok()?.dyn_into().ok()?;
    JsFuture::from(promise).await.ok()
}

/// The session, its storage and its channels; absent until the stored packs have loaded.
struct Driver {
    storage: BrowserStorage,
    session: Option<Session>,
    commands: Sender<Command>,
    incoming: Receiver<Command>,
    outgoing: Sender<Event>,
    draining: bool,
}

thread_local! {
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
    /// Commands queued before the stored packs finished loading.
    static EARLY: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
}

/// Opens the packs the page keeps below `root`, hands their stored selection to the runtime,
/// then handles commands from `commands` as [`wake`] reports them.
pub(super) fn start(
    root: PathBuf,
    commands: Sender<Command>,
    incoming: Receiver<Command>,
    outgoing: Sender<Event>,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let storage = BrowserStorage {
            root,
            files: Arc::default(),
        };
        let names: Vec<String> = match call("names", &[]).await {
            Some(names) => Array::from(&names)
                .iter()
                .filter_map(|name| name.as_string())
                .collect(),
            None => Vec::new(),
        };
        storage.files().stored.extend(names.iter().cloned());
        storage
            .load(names.into_iter().filter(|name| !archive(name)))
            .await;
        let mut session = Session::open(Box::new(storage.clone()), &outgoing);
        if let Some(session) = &mut session {
            storage.load(session.reads(None)).await;
            session.start(&outgoing);
            storage.release_archives();
        }
        for command in EARLY.take() {
            let _ = commands.send(command);
        }
        DRIVER.set(Some(Driver {
            storage,
            session,
            commands,
            incoming,
            outgoing,
            draining: false,
        }));
        wake();
    });
}

/// Queues `command` from outside the app's frame systems, as a page drop.
pub(super) fn queue(command: Command) {
    DRIVER.with_borrow(|driver| match driver {
        Some(driver) => {
            let _ = driver.commands.send(command);
        }
        None => EARLY.with_borrow_mut(|early| early.push(command)),
    });
    wake();
}

/// Handles queued commands between frames, one at a time.
pub(super) fn wake() {
    let start = DRIVER.with_borrow_mut(|driver| {
        driver
            .as_mut()
            .filter(|driver| !driver.draining)
            .is_some_and(|driver| {
                driver.draining = true;
                true
            })
    });
    if start {
        wasm_bindgen_futures::spawn_local(drain());
    }
}

/// Handles every queued command, loading what each reads first; an Import opens the page's file
/// picker instead of a desktop one.
async fn drain() {
    loop {
        let Some((command, storage, mut session, outgoing)) = DRIVER.with_borrow_mut(|driver| {
            let driver = driver.as_mut()?;
            let next = driver
                .session
                .is_some()
                .then(|| driver.incoming.try_recv().ok())
                .flatten();
            let Some(command) = next else {
                driver.draining = false;
                return None;
            };
            let session = driver.session.take()?;
            Some((
                command,
                driver.storage.clone(),
                session,
                driver.outgoing.clone(),
            ))
        }) else {
            return;
        };
        let commands = match command {
            Command::Action {
                action: launcher::global_resources::Action::Import,
                ..
            } => picked().await,
            command => {
                storage.load(session.reads(Some(&command))).await;
                vec![command]
            }
        };
        for command in commands {
            session.handle(command, &outgoing);
        }
        storage.release_archives();
        DRIVER.with_borrow_mut(|driver| {
            if let Some(driver) = driver {
                driver.session = Some(session);
            }
        });
    }
}

/// The files the player picks in the page's file picker, as imports.
async fn picked() -> Vec<Command> {
    let Some(files) = call("pick", &[]).await else {
        return Vec::new();
    };
    Array::from(&files)
        .iter()
        .filter_map(|file| {
            let file = Array::from(&file);
            let name = file.get(0).as_string()?;
            let bytes = file.get(1).dyn_into::<Uint8Array>().ok()?.to_vec();
            Some(Command::ImportArchive(name, bytes))
        })
        .collect()
}
