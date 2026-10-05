//! A single coalescing persistence lane for the selected component's companion.

use std::{
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

#[derive(Default)]
struct Pending {
    json: Option<String>,
    error: Option<String>,
    closing: bool,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    wake: Condvar,
}

pub(super) struct SettingsWriter {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    destination: PathBuf,
    active: bool,
}

impl SettingsWriter {
    #[cfg(test)]
    pub fn new(component: &Path) -> io::Result<Self> {
        let mut writer = Self::prepare(component)?;
        writer.activate();
        Ok(writer)
    }

    /// Creates the disk lane off-frame but permits no writes until publication.
    pub fn prepare(component: &Path) -> io::Result<Self> {
        let path = component.with_extension("settings.json");
        // Resolve the directory without following a companion-file link: writes
        // atomically replace the selected companion entry, as the ordinary loader does.
        let destination = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?
            .join(
                path.file_name()
                    .ok_or_else(|| io::Error::other("settings file unavailable"))?,
            );
        let mut writer = Self::start(destination, write_companion)?;
        writer.active = false;
        Ok(writer)
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }

    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    fn start(
        destination: PathBuf,
        write: impl Fn(&Path, &str) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let shared = Arc::new(Shared::default());
        let thread_shared = Arc::clone(&shared);
        let worker_destination = destination.clone();
        let worker = thread::Builder::new()
            .name("cinnabar-mod-settings".into())
            .spawn(move || {
                loop {
                    let json = {
                        let mut pending = thread_shared
                            .pending
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        while pending.json.is_none() && !pending.closing {
                            pending = thread_shared
                                .wake
                                .wait(pending)
                                .unwrap_or_else(|error| error.into_inner());
                        }
                        let Some(json) = pending.json.take() else {
                            break;
                        };
                        json
                    };
                    if let Err(error) = write(&worker_destination, &json) {
                        let mut pending = thread_shared
                            .pending
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        pending.error = Some(format!(
                            "persist mod settings {}: {error}",
                            worker_destination.display()
                        ));
                    }
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
            destination,
            active: true,
        })
    }

    /// Queues the latest committed value; at most one document waits behind an active write.
    pub fn submit(&self, json: String) {
        if !self.active {
            return;
        }
        debug_assert!(json.len() <= mod_api::MAX_SETTINGS_BYTES);
        self.shared
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .json = Some(json);
        self.shared.wake.notify_one();
    }

    pub fn take_error(&self) -> Option<String> {
        self.shared
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .error
            .take()
    }
}

impl Drop for SettingsWriter {
    fn drop(&mut self) {
        self.shared
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closing = true;
        self.shared.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn write_companion(destination: &Path, json: &str) -> io::Result<()> {
    static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);
    let temporary = destination.with_extension(format!(
        "pending-{}-{}",
        std::process::id(),
        NEXT_WRITE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, destination)
    })();
    if created && result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn latest_pending_document_is_coalesced_and_flushed_on_shutdown() {
        let (started, waiting) = mpsc::sync_channel(0);
        let (release, released) = mpsc::sync_channel(0);
        let writes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&writes);
        let writer = SettingsWriter::start(
            PathBuf::from("selected.component.settings.json"),
            move |_, json| {
                let first = recorded.lock().unwrap().is_empty();
                if first {
                    started.send(()).unwrap();
                    released.recv().unwrap();
                }
                recorded.lock().unwrap().push(json.to_owned());
                Ok(())
            },
        )
        .unwrap();
        writer.submit("{\"cps\":1}".into());
        waiting.recv().unwrap();
        for cps in 2..=30 {
            writer.submit(format!("{{\"cps\":{cps}}}"));
        }
        release.send(()).unwrap();
        drop(writer);
        assert_eq!(*writes.lock().unwrap(), ["{\"cps\":1}", "{\"cps\":30}"]);
    }

    #[test]
    fn only_fixed_companion_is_atomically_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let component = directory.path().join("selected.component.wasm");
        let destination = component.with_extension("settings.json");
        let unrelated = directory.path().join("other.settings.json");
        std::fs::write(&component, b"component bytes").unwrap();
        std::fs::write(&destination, "{\"cps\":12}").unwrap();
        std::fs::write(&unrelated, "untouched").unwrap();
        let writer = SettingsWriter::new(&component).unwrap();
        writer.submit("{\"cps\":25}".into());
        drop(writer);
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            "{\"cps\":25}"
        );
        assert_eq!(std::fs::read(&component).unwrap(), b"component bytes");
        assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "untouched");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn companion_symlink_is_replaced_without_writing_its_outside_target() {
        let selected = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let component = selected.path().join("selected.component.wasm");
        let companion = component.with_extension("settings.json");
        let target = outside.path().join("private.json");
        std::fs::write(&target, "outside target unchanged").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &companion).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_file(&target, &companion) {
            if error.raw_os_error() == Some(1314) {
                eprintln!(
                    "Skipping symlink fixture: SeCreateSymbolicLinkPrivilege is unavailable."
                );
                return;
            }
            panic!("create companion symlink fixture: {error}");
        }
        let writer = SettingsWriter::new(&component).unwrap();
        writer.submit("{\"cps\":25}".into());
        drop(writer);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "outside target unchanged"
        );
        assert_eq!(std::fs::read_to_string(&companion).unwrap(), "{\"cps\":25}");
        assert!(
            !std::fs::symlink_metadata(companion)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn persistence_failure_is_reported_once_without_retry_spam() {
        let (failed, failure) = mpsc::sync_channel(0);
        let attempts = Arc::new(AtomicU64::new(0));
        let recorded = Arc::clone(&attempts);
        let writer = SettingsWriter::start(
            PathBuf::from("selected.component.settings.json"),
            move |_, _| {
                recorded.fetch_add(1, Ordering::Relaxed);
                failed.send(()).unwrap();
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "fixture denies writes",
                ))
            },
        )
        .unwrap();
        writer.submit("{}".into());
        failure.recv().unwrap();
        // Joining waits for the error to be retained without relying on wall-clock timing.
        writer.shared.pending.lock().unwrap().closing = true;
        writer.shared.wake.notify_one();
        let mut writer = writer;
        writer.worker.take().unwrap().join().unwrap();
        assert!(
            writer
                .take_error()
                .unwrap()
                .contains("fixture denies writes")
        );
        assert!(writer.take_error().is_none());
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }
}
