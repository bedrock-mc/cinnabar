//! Retains a verified package while every manifest-selected file has the same change identity.

use super::{Directory, Package};
use crate::local_files::{CONFIRMATION_DELAY, Identity};
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

/// A worker-owned verified package cache, checked through a fresh directory handle each poll.
#[derive(Default)]
pub struct Observation {
    cached: Option<(PathBuf, Result<Arc<Package>, String>)>,
    files: Vec<Dependency>,
    confirm_after: Option<Instant>,
}

impl Observation {
    /// Reuses an immutable package only when all its inputs still select the same unchanged files.
    pub fn read(&mut self, path: &Path) -> Result<Arc<Package>> {
        self.read_with(path, |opened, limit| opened.read(limit))
    }

    /// Reads changed package inputs through one directory snapshot and preserves unchanged failures.
    pub(super) fn read_with(
        &mut self,
        path: &Path,
        read: impl FnMut(&mut crate::local_files::Opened, usize) -> std::io::Result<Vec<u8>>,
    ) -> Result<Arc<Package>> {
        self.read_at(path, Instant::now(), read)
    }

    /// Confirms package inputs once after their initial timestamp collision window closes.
    pub(super) fn read_at(
        &mut self,
        path: &Path,
        now: Instant,
        mut read: impl FnMut(&mut crate::local_files::Opened, usize) -> std::io::Result<Vec<u8>>,
    ) -> Result<Arc<Package>> {
        let root = match Directory::open(path).context("open mod package directory") {
            Ok(root) => root,
            Err(error) => {
                self.cached = None;
                self.files.clear();
                self.confirm_after = None;
                return Err(error);
            }
        };
        let mut confirming = false;
        if let Some((previous, package)) = &self.cached
            && previous == path
            && self.unchanged(&root)
        {
            confirming = self.confirm_after.is_some_and(|deadline| now >= deadline);
            if !confirming {
                return package.clone().map_err(anyhow::Error::msg);
            }
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
            let before = Identity::of(&opened.file).inspect_err(|_| reusable = false)?;
            let bytes = read(&mut opened, limit);
            if bytes.is_err() {
                reusable = false;
            }
            let after = Identity::of(&opened.file).inspect_err(|_| reusable = false)?;
            if before != after {
                reusable = false;
                return Err(std::io::Error::other(
                    "package source changed while being read",
                ));
            }
            if let Some(identity) = after {
                self.files.push(Dependency {
                    path: path.to_owned(),
                    selected: Some(identity),
                });
            } else {
                reusable = false;
            }
            bytes
        })
        .map(Arc::new)
        .map_err(|error| format!("{error:#}"));
        if let (Some((_, Ok(previous))), Ok(current)) = (&previous, &package)
            && previous.digest == current.digest
        {
            package = Ok(previous.clone());
        }
        if reusable {
            self.cached = Some((path.to_owned(), package.clone()));
            self.confirm_after = (!confirming).then_some(now + CONFIRMATION_DELAY);
        } else {
            self.files.clear();
            self.confirm_after = None;
        }
        package.map_err(anyhow::Error::msg)
    }

    /// Checks every dependency through the current root, refusing changed or linked parents/leaves.
    fn unchanged(&self, root: &Directory) -> bool {
        self.files.iter().all(|dependency| {
            match (&dependency.selected, root.open_file(&dependency.path)) {
                (Some(previous), Ok(opened)) => Identity::of(&opened.file)
                    .is_ok_and(|current| current.as_ref() == Some(previous)),
                (None, Err(error)) => error.kind() == std::io::ErrorKind::NotFound,
                _ => false,
            }
        })
    }
}
