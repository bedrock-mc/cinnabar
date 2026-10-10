//! The disk tier: one directory of downloaded images per surface, named by the SHA-256 of
//! their URL and evicted least recently used beyond the surface's file and byte bounds.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use super::{Surface, client, download, image_extension};

/// Downloads in flight at once per folder, across every cache instance using it.
const MAX_IN_FLIGHT: usize = 8;
/// Callers waiting for a download slot per folder; later ones are refused rather than queued.
const MAX_WAITING: usize = 256;
/// How long a failed URL is answered from memory before it is downloaded again.
const RETRY_AFTER: Duration = Duration::from_secs(5 * 60);
/// Failed URLs remembered per folder; the oldest are forgotten first.
const MAX_FAILED: usize = 256;
/// Extensions a signature-named file may carry.
const EXTENSIONS: [&str; 4] = [".png", ".jpg", ".gif", ".bmp"];

/// A bounded on-disk image cache for one surface. Use one per tokio runtime: its client's
/// pooled connections are driven by the runtime that opened them.
#[derive(Clone)]
pub struct ImageDirectory(Arc<Inner>);

struct Inner {
    dir: PathBuf,
    surface: Surface,
    client: Option<reqwest::Client>,
    folder: Arc<Folder>,
}

/// State every cache instance on one folder shares within this process.
struct Folder {
    slots: Semaphore,
    waiting: AtomicUsize,
    /// Orders publication and eviction.
    writes: Mutex<()>,
    failed: Mutex<Failures>,
}

/// Recently failed URLs, so a dead host is not asked again on every feed round.
#[derive(Default)]
struct Failures {
    at: HashMap<String, Instant>,
    order: VecDeque<String>,
}

impl Failures {
    fn recent(&mut self, url: &str) -> bool {
        match self.at.get(url) {
            Some(at) if at.elapsed() < RETRY_AFTER => true,
            Some(_) => {
                self.at.remove(url);
                self.order.retain(|old| old != url);
                false
            }
            None => false,
        }
    }

    fn record(&mut self, url: &str) {
        if self.at.insert(url.to_owned(), Instant::now()).is_none() {
            self.order.push_back(url.to_owned());
        }
        while self.order.len() > MAX_FAILED {
            if let Some(oldest) = self.order.pop_front() {
                self.at.remove(&oldest);
            }
        }
    }
}

/// The shared state for `dir`, created on first use.
fn folder(dir: &Path) -> Arc<Folder> {
    static FOLDERS: OnceLock<Mutex<HashMap<PathBuf, Arc<Folder>>>> = OnceLock::new();
    let mut folders = FOLDERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let folder = folders.entry(dir.to_path_buf()).or_insert_with(|| {
        Arc::new(Folder {
            slots: Semaphore::new(MAX_IN_FLIGHT),
            waiting: AtomicUsize::new(0),
            writes: Mutex::new(()),
            failed: Mutex::new(Failures::default()),
        })
    });
    Arc::clone(folder)
}

impl ImageDirectory {
    /// A cache rooted at `dir`, created on first download.
    pub fn new(dir: PathBuf, surface: Surface) -> Self {
        Self(Arc::new(Inner {
            folder: folder(&dir),
            dir,
            client: client(&surface),
            surface,
        }))
    }

    /// A cache whose downloads go through `client`, so tests can substitute its resolver.
    #[cfg(test)]
    fn with_client(dir: PathBuf, surface: Surface, client: Option<reqwest::Client>) -> Self {
        Self(Arc::new(Inner {
            folder: folder(&dir),
            dir,
            client,
            surface,
        }))
    }

    /// The cached file for `url`, downloading it when absent; `None` for a refused URL, a
    /// payload that is not a PNG, JPEG, GIF or BMP, or a download that failed recently.
    pub async fn fetch(&self, url: &str) -> Option<PathBuf> {
        let inner = &self.0;
        let stem = self.stem(url)?;
        if let Some(path) = self.cached(&stem) {
            return Some(path);
        }
        if self.failed_recently(url) {
            return None;
        }
        let waiting = Waiting::enter(&inner.folder.waiting)?;
        let _slot = inner.folder.slots.acquire().await.ok()?;
        drop(waiting);
        // Another caller may have stored it while this one waited.
        if let Some(path) = self.cached(&stem) {
            return Some(path);
        }
        let stored = match download(inner.client.as_ref()?, &inner.surface, url, true).await {
            Some(body) => image_extension(&body).and_then(|extension| {
                self.store(&stem, inner.surface.extension.unwrap_or(extension), &body)
            }),
            None => None,
        };
        if stored.is_none() {
            lock(&inner.folder.failed).record(url);
        }
        stored
    }

    /// The file already cached for `url`, without touching the network.
    pub fn cached_path(&self, url: &str) -> Option<PathBuf> {
        self.cached(&self.stem(url)?)
    }

    /// Whether `url` failed within [`RETRY_AFTER`], so fetching it now would return `None`.
    pub fn failed_recently(&self, url: &str) -> bool {
        lock(&self.0.folder.failed).recent(url)
    }

    /// Where an accepted URL's file lives, without its extension.
    fn stem(&self, url: &str) -> Option<PathBuf> {
        self.0
            .surface
            .accepts(url)
            .then(|| self.0.dir.join(hex(&Sha256::digest(url.as_bytes()))))
    }

    /// Fetches every URL with at most [`MAX_IN_FLIGHT`] downloads running, in input order;
    /// downloads still unfinished after `budget` are abandoned and left `None`.
    pub async fn fetch_all(&self, urls: Vec<String>, budget: Duration) -> Vec<Option<PathBuf>> {
        let deadline = tokio::time::Instant::now() + budget;
        let mut results = vec![None; urls.len()];
        let mut pending = urls.into_iter().enumerate();
        let mut running = tokio::task::JoinSet::new();
        loop {
            while running.len() < MAX_IN_FLIGHT
                && let Some((index, url)) = pending.next()
            {
                let directory = self.clone();
                running.spawn(async move { (index, directory.fetch(&url).await) });
            }
            let Ok(Some(done)) = tokio::time::timeout_at(deadline, running.join_next()).await
            else {
                return results;
            };
            if let Ok((index, path)) = done {
                results[index] = path;
            }
        }
    }

    /// Removes the least recently used files beyond the surface's bounds.
    pub fn prune(&self) {
        let _writes = lock(&self.0.folder.writes);
        self.evict();
    }

    /// A previous download at `stem`, refreshed as recently used, if it still looks like an image.
    fn cached(&self, stem: &Path) -> Option<PathBuf> {
        let candidates: &[&str] = match &self.0.surface.extension {
            Some(extension) => std::slice::from_ref(extension),
            None => &EXTENSIONS,
        };
        candidates.iter().find_map(|extension| {
            let path = with_extension(stem, extension);
            let mut file = fs::File::open(&path).ok()?;
            let metadata = file.metadata().ok()?;
            if !metadata.is_file() || metadata.len() > self.0.surface.max_bytes as u64 {
                return None;
            }
            let mut header = Vec::with_capacity(8);
            (&mut file).take(8).read_to_end(&mut header).ok()?;
            image_extension(&header)?;
            let _ = fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(SystemTime::now()));
            Some(path)
        })
    }

    /// Publishes `body` atomically under `stem`, then enforces the directory bounds.
    fn store(&self, stem: &Path, extension: &str, body: &[u8]) -> Option<PathBuf> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let _writes = lock(&self.0.folder.writes);
        create_private_dir(&self.0.dir).ok()?;
        let temporary = self.0.dir.join(format!(
            ".image-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let target = with_extension(stem, extension);
        let written = create_private_file(&temporary)
            .and_then(|mut file| file.write_all(body).and_then(|()| file.sync_all()))
            .and_then(|()| fs::rename(&temporary, &target));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
            return None;
        }
        self.evict();
        Some(target)
    }

    /// Deletes the oldest visible files until the count and byte bounds hold.
    fn evict(&self) {
        let Ok(entries) = fs::read_dir(&self.0.dir) else {
            return;
        };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                metadata.is_file().then(|| {
                    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    (modified, metadata.len(), entry.path())
                })
            })
            .collect();
        files.sort_by_key(|(modified, ..)| *modified);
        let surface = &self.0.surface;
        let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
        let mut count = files.len();
        for (_, size, path) in files {
            if count <= surface.max_files
                && (surface.max_dir_bytes == 0 || total <= surface.max_dir_bytes)
            {
                break;
            }
            let _ = fs::remove_file(path);
            count -= 1;
            total = total.saturating_sub(size);
        }
    }
}

/// One caller counted against [`MAX_WAITING`] until it gets a slot or is dropped.
struct Waiting<'a>(&'a AtomicUsize);

impl<'a> Waiting<'a> {
    fn enter(count: &'a AtomicUsize) -> Option<Self> {
        let waiting = Self(count);
        (count.fetch_add(1, Ordering::AcqRel) < MAX_WAITING).then_some(waiting)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

fn with_extension(stem: &Path, extension: &str) -> PathBuf {
    let mut path = stem.as_os_str().to_owned();
    path.push(extension);
    PathBuf::from(path)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

#[cfg(test)]
mod tests;
