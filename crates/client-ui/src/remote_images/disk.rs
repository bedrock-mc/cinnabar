//! The disk tier: one directory of downloaded images per surface, named by the SHA-256 of
//! their URL and evicted least recently used beyond the surface's file and byte bounds.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;

use super::{Surface, client, download, image_extension};

/// Downloads in flight at once per directory.
const MAX_IN_FLIGHT: usize = 8;
/// Callers waiting for a download slot; later ones are refused rather than queued.
const MAX_WAITING: usize = 256;
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
    slots: Semaphore,
    waiting: AtomicUsize,
    /// Orders publication and eviction within this process.
    writes: Mutex<()>,
}

impl ImageDirectory {
    /// A cache rooted at `dir`, created on first download.
    pub fn new(dir: PathBuf, surface: Surface) -> Self {
        Self(Arc::new(Inner {
            dir,
            client: client(&surface),
            surface,
            slots: Semaphore::new(MAX_IN_FLIGHT),
            waiting: AtomicUsize::new(0),
            writes: Mutex::new(()),
        }))
    }

    /// The cached file for `url`, downloading it when absent; `None` for a refused URL,
    /// a failed download or a payload that is not a PNG, JPEG, GIF or BMP.
    pub async fn fetch(&self, url: &str) -> Option<PathBuf> {
        let inner = &self.0;
        if !inner.surface.accepts(url) {
            return None;
        }
        let stem = inner.dir.join(hex(&Sha256::digest(url.as_bytes())));
        if let Some(path) = self.cached(&stem) {
            return Some(path);
        }
        let waiting = Waiting::enter(&inner.waiting)?;
        let _slot = inner.slots.acquire().await.ok()?;
        drop(waiting);
        // Another caller may have stored it while this one waited.
        if let Some(path) = self.cached(&stem) {
            return Some(path);
        }
        let body = download(inner.client.as_ref()?, &inner.surface, url, true).await?;
        let extension = image_extension(&body)?;
        self.store(&stem, inner.surface.extension.unwrap_or(extension), &body)
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
        let _writes = self.0.writes.lock().unwrap_or_else(|p| p.into_inner());
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
        let _writes = self.0.writes.lock().unwrap_or_else(|p| p.into_inner());
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
