//! Subscriber fingerprints preserve unchanged compiled resources across stack edits.

use super::{pack_reload::PackInputs, resource_packs::PackApplication};
use resource_pack::{PackAdmission, PackDependency, ValidatedPackStack};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Subscriber {
    Blocks,
    Atmosphere,
    Particles,
    Icons,
    Glyphs,
    Entities,
    Ui,
    Sounds,
    Language,
    AimAssist,
}

pub(super) type Dependencies = BTreeMap<Subscriber, BTreeSet<PackDependency>>;

/// Runs one compiler with an independent record of its successful and missing reads.
pub(super) fn compile<T>(
    subscriber: Subscriber,
    stack: &std::sync::Arc<ValidatedPackStack>,
    dependencies: &mut Dependencies,
    build: impl FnOnce(&resource_pack::LayeredPackView) -> T,
) -> T {
    let view = resource_pack::LayeredPackView::tracked(stack.clone());
    let output = build(&view);
    let inputs = view.dependencies().expect("tracked view").snapshot();
    dependencies.insert(subscriber, inputs);
    output
}

/// One subscriber's output and, when compiled rather than reused, its reads; `None` once
/// cancelled before it started.
pub(super) fn compile_part<T>(
    stack: &std::sync::Arc<ValidatedPackStack>,
    cancelled: &(dyn Fn() -> bool + Sync),
    changed: bool,
    subscriber: Subscriber,
    reuse: T,
    build: impl FnOnce(&resource_pack::LayeredPackView) -> T,
) -> Option<(T, Option<BTreeSet<PackDependency>>)> {
    if !changed {
        return Some((reuse, None));
    }
    if cancelled() {
        return None;
    }
    let mut inputs = Dependencies::new();
    let output = compile(subscriber, stack, &mut inputs, build);
    Some((output, inputs.remove(&subscriber)))
}

pub(super) struct Changes {
    /// Whether any file may read differently, so whole-stack facts are read again.
    pub(super) contents: bool,
    pub(super) blocks: bool,
    pub(super) atmosphere: bool,
    pub(super) particles: bool,
    pub(super) icons: bool,
    pub(super) glyphs: bool,
    pub(super) entities: bool,
    pub(super) ui: bool,
    pub(super) sounds: bool,
    pub(super) language: bool,
    pub(super) aim_assist: bool,
}

impl Changes {
    /// Every subscriber compiles from scratch.
    pub(super) const fn all() -> Self {
        Self {
            contents: true,
            blocks: true,
            atmosphere: true,
            particles: true,
            icons: true,
            glyphs: true,
            entities: true,
            ui: true,
            sounds: true,
            language: true,
            aim_assist: true,
        }
    }

    /// For a stack whose every read matches the one `previous` compiled: only the subscribers
    /// that also read StartGame facts compile again, and only when those facts differ.
    pub(super) fn for_inputs(previous: &PackInputs, next: &PackInputs) -> Self {
        Self {
            contents: false,
            blocks: !next.same_blocks(previous),
            atmosphere: false,
            particles: false,
            icons: !next.same_icons(previous),
            glyphs: false,
            entities: false,
            ui: false,
            sounds: false,
            language: false,
            aim_assist: false,
        }
    }

    /// Compares layer contents, including order, rather than pack names or timestamps.
    pub(super) fn between(stack: &ValidatedPackStack, previous: Option<&PackApplication>) -> Self {
        let prior = previous.and_then(|old| match &old.admission {
            PackAdmission::Validated(stack) => Some(stack.as_ref()),
            PackAdmission::None => None,
        });
        let changed = |subscriber| {
            prior.is_none_or(|old| {
                let recorded = previous.and_then(|previous| previous.dependencies.get(&subscriber));
                let fallback;
                let inputs = if let Some(recorded) = recorded {
                    recorded
                } else {
                    fallback = old
                        .packs()
                        .iter()
                        .chain(stack.packs())
                        .flat_map(|pack| pack.files_under("").into_vec())
                        .filter(|path| *path != "manifest.json")
                        .map(|path| PackDependency::File {
                            path: path.to_owned(),
                            limit: resource_pack::MAX_FILE_BYTES,
                        })
                        .collect();
                    &fallback
                };
                dependency_fingerprint(old, inputs) != dependency_fingerprint(stack, inputs)
            })
        };
        let blocks = changed(Subscriber::Blocks);
        Self {
            contents: true,
            blocks,
            atmosphere: changed(Subscriber::Atmosphere),
            particles: changed(Subscriber::Particles),
            icons: blocks || changed(Subscriber::Icons),
            glyphs: changed(Subscriber::Glyphs),
            entities: changed(Subscriber::Entities),
            ui: changed(Subscriber::Ui),
            sounds: changed(Subscriber::Sounds),
            language: changed(Subscriber::Language),
            aim_assist: changed(Subscriber::AimAssist),
        }
    }
}

/// Replays consumed reads; listing an unrelated texture does not consume its pixels.
fn dependency_fingerprint(
    stack: &ValidatedPackStack,
    inputs: &BTreeSet<PackDependency>,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    for pack in stack.packs() {
        let mut layer = Sha256::new();
        let mut populated = false;
        for input in inputs {
            match input {
                PackDependency::File { path, limit } => {
                    if let Ok(Some(bytes)) = pack.read_file_with_limit(path, *limit) {
                        layer.update([0]);
                        hash_part(&mut layer, path.as_bytes());
                        hash_part(&mut layer, &bytes);
                        populated = true;
                    }
                }
                PackDependency::Directory(prefix) => {
                    for path in pack.files_under(prefix) {
                        layer.update([1]);
                        hash_part(&mut layer, prefix.as_bytes());
                        hash_part(&mut layer, path.as_bytes());
                        populated = true;
                    }
                }
                PackDependency::DirectoryWithSuffixes { prefix, suffixes } => {
                    for path in pack.files_under(prefix) {
                        if suffixes.iter().any(|suffix| path.ends_with(suffix)) {
                            layer.update([3]);
                            hash_part(&mut layer, prefix.as_bytes());
                            hash_part(&mut layer, path.as_bytes());
                            populated = true;
                        }
                    }
                }
                PackDependency::Contents(prefix) => {
                    for path in pack.files_under(prefix) {
                        layer.update([2]);
                        hash_part(&mut layer, path.as_bytes());
                        if let Ok(Some(bytes)) = pack.read_file(path) {
                            hash_part(&mut layer, &bytes);
                        }
                        populated = true;
                    }
                }
            }
        }
        if populated {
            hash.update(layer.finalize());
        }
    }
    hash.finalize().into()
}

/// Length framing prevents two adjacent names or payloads from aliasing one input.
fn hash_part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

#[cfg(test)]
#[path = "pack_reload_diff_tests.rs"]
mod tests;
