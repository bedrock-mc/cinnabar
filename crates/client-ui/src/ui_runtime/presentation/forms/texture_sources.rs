//! The host's texture sources for the engine's atlas: the installed vanilla pack on disk, the
//! remote image cache, and the clock and threads its image work runs on.

use std::{io::Read, path::PathBuf, sync::Arc, time::Instant};

use launcher_host::remote_images::{RemoteImages, RemoteState};
use ui_engine::sources::{ImageWork, LocalPack, RemoteImage, RemoteSource};

/// The installed vanilla resource pack under its root directory.
pub(super) struct VanillaPack(PathBuf);

impl VanillaPack {
    pub(super) fn new(root: PathBuf) -> Self {
        Self(root)
    }
}

impl LocalPack for VanillaPack {
    fn file_size(&self, relative: &str) -> Option<u64> {
        let path = vanilla_path(&self.0, relative)?;
        exact_case(&path).then_some(())?;
        Some(std::fs::metadata(&path).ok()?.len())
    }

    fn read(&self, relative: &str, limit: u64) -> Option<Vec<u8>> {
        let path = vanilla_path(&self.0, relative)?;
        let file = std::fs::File::open(path).ok()?;
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes).ok()?;
        (bytes.len() as u64 <= limit).then_some(bytes)
    }
}

/// Downloads through one remote image cache.
struct Downloads(RemoteImages);

impl RemoteSource for Downloads {
    fn state(&self, url: &str) -> RemoteImage {
        match self.0.state(url) {
            RemoteState::Loading => RemoteImage::Loading,
            RemoteState::Ready(generation, bytes) => RemoteImage::Ready(generation, bytes),
            RemoteState::Failed => RemoteImage::Failed,
        }
    }

    fn touch(&self, url: &str) {
        self.0.touch(url);
    }
}

/// A fresh remote image cache for one engine.
pub(super) fn downloads() -> Arc<dyn RemoteSource> {
    Arc::new(Downloads(RemoteImages::default()))
}

/// Background workers, with inline decodes bounded on the monotonic clock.
pub(super) fn image_work() -> ImageWork {
    let origin = Instant::now();
    ImageWork {
        clock: Some(Arc::new(move || origin.elapsed())),
        workers: true,
    }
}

/// Resolves a pack-relative file while refusing parent components and linked escapes.
fn vanilla_path(root: &std::path::Path, relative: &str) -> Option<std::path::PathBuf> {
    use std::path::{Component, Path};
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || relative.as_os_str().is_empty()
    {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let path = root.join(relative);
    let canonical = path.canonicalize().ok()?;
    canonical.starts_with(&root).then_some(path)
}

/// Whether `path`'s file name exists spelled exactly so, as the client's asset
/// index matches even on a case-insensitive file system.
fn exact_case(path: &std::path::Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    std::fs::read_dir(parent).is_ok_and(|mut entries| {
        entries.any(|entry| entry.is_ok_and(|entry| entry.file_name() == name))
    })
}

#[cfg(test)]
mod tests;
