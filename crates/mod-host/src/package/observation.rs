//! Retains a verified package while every manifest-selected file has the same change identity.

use super::{Directory, Package};
use crate::local_files::{ContentDeadline, Identity};
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

struct Dependency {
    path: PathBuf,
    selected: Option<Identity>,
}

struct Cached {
    path: PathBuf,
    package: Result<Arc<Package>, String>,
    deadline: ContentDeadline,
}

/// A worker-owned verified package cache, checked through a fresh directory handle each poll.
#[derive(Default)]
pub struct Observation {
    cached: Option<Cached>,
    files: Vec<Dependency>,
}

impl Observation {
    /// Checks metadata each poll and revalidates all content at the shared fallback deadline.
    pub fn read(&mut self, path: &Path, now: Instant) -> Result<Arc<Package>> {
        self.read_at(path, now, Identity::of, |opened, limit| opened.read(limit))
    }

    /// Reads changed or expired package inputs through one secure directory snapshot.
    pub(super) fn read_at(
        &mut self,
        path: &Path,
        now: Instant,
        identify: impl Fn(&std::fs::File) -> std::io::Result<Identity>,
        mut read: impl FnMut(&mut crate::local_files::Opened, usize) -> std::io::Result<Vec<u8>>,
    ) -> Result<Arc<Package>> {
        let root = match Directory::open(path).context("open mod package directory") {
            Ok(root) => root,
            Err(error) => {
                self.cached = None;
                self.files.clear();
                return Err(error);
            }
        };
        if let Some(cached) = &self.cached
            && cached.path == path
            && cached.deadline.allows_reuse(now)
            && self.unchanged(&root, &identify)
        {
            return cached.package.clone().map_err(anyhow::Error::msg);
        }
        let previous = self.cached.take();
        self.files.clear();
        let mut reusable = true;
        let mut package = Package::read_files(|path, limit| {
            let mut opened = match root.open_file(path) {
                Ok(opened) => opened,
                Err(error) => {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        self.files.push(Dependency {
                            path: path.to_owned(),
                            selected: None,
                        });
                    } else {
                        reusable = false;
                    }
                    return Err(error);
                }
            };
            let before = identify(&opened.file).inspect_err(|_| reusable = false)?;
            let bytes = read(&mut opened, limit);
            if bytes.is_err() {
                reusable = false;
            }
            let after = identify(&opened.file).inspect_err(|_| reusable = false)?;
            if before != after {
                reusable = false;
                return Err(std::io::Error::other(
                    "package source changed while being read",
                ));
            }
            self.files.push(Dependency {
                path: path.to_owned(),
                selected: Some(after),
            });
            bytes
        })
        .map(Arc::new)
        .map_err(|error| format!("{error:#}"));
        if let (
            Some(Cached {
                package: Ok(previous),
                ..
            }),
            Ok(current),
        ) = (&previous, &package)
            && previous.digest == current.digest
        {
            package = Ok(previous.clone());
        }
        if reusable {
            self.cached = Some(Cached {
                path: path.to_owned(),
                package: package.clone(),
                deadline: ContentDeadline::new(now),
            });
        } else {
            self.files.clear();
        }
        package.map_err(anyhow::Error::msg)
    }

    /// Checks every dependency through the current root, refusing changed or linked parents/leaves.
    fn unchanged(
        &self,
        root: &Directory,
        identify: &impl Fn(&std::fs::File) -> std::io::Result<Identity>,
    ) -> bool {
        self.files.iter().all(|dependency| {
            match (&dependency.selected, root.open_file(&dependency.path)) {
                (Some(previous), Ok(opened)) => {
                    identify(&opened.file).is_ok_and(|current| current == *previous)
                }
                (None, Err(error)) => error.kind() == std::io::ErrorKind::NotFound,
                _ => false,
            }
        })
    }
}
