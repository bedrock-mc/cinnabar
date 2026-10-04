//! Precedence-ordered reads over an admitted stack.
//!
//! The last `ResourcePackStack` entry wins: the client builds its stack in list
//! order (a pack's dependencies first) and resolves a resource from the highest
//! index down. Confirm this vanilla behavior with a live two-pack capture.

use std::{collections::BTreeSet, sync::Arc};

use crate::{AdmissionError, MAX_FILE_BYTES, PackDependencies, ValidatedPack, ValidatedPackStack};

/// A read-only merged namespace over one session's admitted packs.
#[derive(Clone, Debug)]
pub struct LayeredPackView {
    stack: Arc<ValidatedPackStack>,
    dependencies: Option<PackDependencies>,
}

impl LayeredPackView {
    #[must_use]
    pub const fn new(stack: Arc<ValidatedPackStack>) -> Self {
        Self {
            stack,
            dependencies: None,
        }
    }

    /// Records only the inputs consumed by this subscriber's compilation.
    pub fn tracked(stack: Arc<ValidatedPackStack>) -> Self {
        Self {
            stack,
            dependencies: Some(PackDependencies::default()),
        }
    }

    /// Returns this compilation's dependency recorder, if tracking was requested.
    pub fn dependencies(&self) -> Option<&PackDependencies> {
        self.dependencies.as_ref()
    }

    #[must_use]
    pub fn stack(&self) -> &ValidatedPackStack {
        &self.stack
    }

    /// Shares admitted archives with a separate subscriber's worker view.
    pub fn shared_stack(&self) -> Arc<ValidatedPackStack> {
        self.stack.clone()
    }

    /// Returns the winning copy of `path`. A pack whose copy cannot be read is
    /// skipped so the next layer down can still supply it.
    #[must_use]
    pub fn read(&self, path: &str) -> Option<Box<[u8]>> {
        self.read_capped(path, MAX_FILE_BYTES)
    }

    /// Like [`read`](Self::read) but reads at most `limit` uncompressed bytes; a
    /// copy over the limit is skipped, so a lower layer's copy can still win.
    #[must_use]
    pub fn read_capped(&self, path: &str, limit: u64) -> Option<Box<[u8]>> {
        if let Some(dependencies) = &self.dependencies {
            dependencies.file(path, limit);
        }
        self.stack
            .packs()
            .iter()
            .rev()
            .find_map(|pack| pack.read_file_with_limit(path, limit).ok().flatten())
    }

    /// Every readable copy of `path`, lowest precedence first.
    #[must_use]
    pub fn read_layers(&self, path: &str) -> Vec<Box<[u8]>> {
        self.read_layers_capped(path, MAX_FILE_BYTES).collect()
    }

    /// Records even empty-stack lookups while streaming bounded layer candidates.
    pub fn read_layers_capped<'a>(
        &'a self,
        path: &'a str,
        limit: u64,
    ) -> impl DoubleEndedIterator<Item = Box<[u8]>> + 'a {
        if let Some(dependencies) = &self.dependencies {
            dependencies.file(path, limit);
        }
        self.stack
            .packs()
            .iter()
            .filter_map(move |pack| pack.read_file_with_limit(path, limit).ok().flatten())
    }

    /// Admitted packs, lowest precedence first.
    pub fn layers(&self) -> impl DoubleEndedIterator<Item = PackLayer<'_>> {
        self.stack.packs().iter().map(|pack| PackLayer {
            pack,
            dependencies: self.dependencies.as_ref(),
        })
    }

    /// Records that a later consumer may read any file under `prefix` after compilation ends.
    pub fn track_contents(&self, prefix: &str) {
        if let Some(dependencies) = &self.dependencies {
            dependencies.contents(prefix);
        }
    }

    /// Lists the union of logical files under `prefix` in lexical order.
    #[must_use]
    pub fn list(&self, prefix: &str) -> Vec<&str> {
        if let Some(dependencies) = &self.dependencies {
            dependencies.directory(prefix);
        }
        let mut paths = BTreeSet::new();
        for pack in self.stack.packs() {
            paths.extend(pack.files_under(prefix).iter().copied());
        }
        paths.into_iter().collect()
    }

    /// Lists only matching suffixes, recording that filtered namespace as the dependency.
    #[must_use]
    pub fn list_with_suffixes(&self, prefix: &str, suffixes: &[&str]) -> Vec<&str> {
        if let Some(dependencies) = &self.dependencies {
            dependencies.directory_with_suffixes(prefix, suffixes);
        }
        let mut paths = BTreeSet::new();
        for pack in self.stack.packs() {
            paths.extend(
                pack.files_under(prefix)
                    .iter()
                    .copied()
                    .filter(|path| suffixes.iter().any(|suffix| path.ends_with(suffix))),
            );
        }
        paths.into_iter().collect()
    }
}

/// A layer read shares the parent view's dependency recorder.
pub struct PackLayer<'a> {
    pack: &'a ValidatedPack,
    dependencies: Option<&'a PackDependencies>,
}

impl<'a> PackLayer<'a> {
    /// Lists names in the layer while retaining the pack's original precedence.
    pub fn files_under(&self, prefix: &str) -> Box<[&'a str]> {
        if let Some(dependencies) = self.dependencies {
            dependencies.directory(prefix);
        }
        self.pack.files_under(prefix)
    }

    /// Records successful and missing layer reads so a newly added override invalidates them.
    pub fn read_file(&self, path: &str) -> Result<Option<Box<[u8]>>, AdmissionError> {
        if let Some(dependencies) = self.dependencies {
            dependencies.file(path, MAX_FILE_BYTES);
        }
        self.pack.read_file(path)
    }

    /// Records a bounded layer read, including missing or oversized overrides.
    pub fn read_file_with_limit(
        &self,
        path: &str,
        limit: u64,
    ) -> Result<Option<Box<[u8]>>, AdmissionError> {
        if let Some(dependencies) = self.dependencies {
            dependencies.file(path, limit);
        }
        self.pack.read_file_with_limit(path, limit)
    }
}
