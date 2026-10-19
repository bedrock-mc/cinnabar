//! Subscriber fingerprints preserve unchanged compiled resources across stack edits.

use crate::PackInputs;
use rayon::prelude::*;
use resource_pack::{PackAdmission, PackDependency, ValidatedPackStack};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Subscriber {
    Blocks,
    BlockEntityArt,
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

impl Subscriber {
    /// A profiler zone named for this subscriber, since zone names must be static.
    fn zone(self) -> tracing::Span {
        match self {
            Self::Blocks => tracing::info_span!("pack.compile.blocks"),
            Self::BlockEntityArt => tracing::info_span!("pack.compile.block_entity_art"),
            Self::Atmosphere => tracing::info_span!("pack.compile.atmosphere"),
            Self::Particles => tracing::info_span!("pack.compile.particles"),
            Self::Icons => tracing::info_span!("pack.compile.icons"),
            Self::Glyphs => tracing::info_span!("pack.compile.glyphs"),
            Self::Entities => tracing::info_span!("pack.compile.entities"),
            Self::Ui => tracing::info_span!("pack.compile.ui"),
            Self::Sounds => tracing::info_span!("pack.compile.sounds"),
            Self::Language => tracing::info_span!("pack.compile.language"),
            Self::AimAssist => tracing::info_span!("pack.compile.aim_assist"),
        }
    }
}

pub type Dependencies = BTreeMap<Subscriber, BTreeSet<PackDependency>>;

/// Runs one compiler with an independent record of its successful and missing reads.
pub fn compile<T>(
    subscriber: Subscriber,
    stack: &std::sync::Arc<ValidatedPackStack>,
    dependencies: &mut Dependencies,
    build: impl FnOnce(&resource_pack::LayeredPackView) -> T,
) -> T {
    let zone = subscriber.zone();
    let _timing = bridge::join_profile::JoinProfile::current().stage(
        zone.metadata()
            .map_or("pack.compile", |metadata| metadata.name()),
    );
    let _zone = zone.entered();
    let view = resource_pack::LayeredPackView::tracked(stack.clone());
    let output = build(&view);
    let inputs = view.dependencies().expect("tracked view").snapshot();
    dependencies.insert(subscriber, inputs);
    output
}

/// One subscriber's output and, when compiled rather than reused, its reads; `None` once
/// cancelled before it started.
pub fn compile_part<T>(
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Changes {
    /// Whether any file may read differently, so whole-stack facts are read again.
    pub contents: bool,
    pub blocks: bool,
    pub block_entity_art: bool,
    pub atmosphere: bool,
    pub particles: bool,
    pub icons: bool,
    pub glyphs: bool,
    pub entities: bool,
    pub ui: bool,
    pub sounds: bool,
    pub language: bool,
    pub aim_assist: bool,
}

impl Changes {
    /// Every subscriber compiles from scratch.
    pub const fn all() -> Self {
        Self {
            contents: true,
            blocks: true,
            block_entity_art: true,
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
    pub fn for_inputs(previous: &PackInputs, next: &PackInputs) -> Self {
        Self {
            contents: false,
            blocks: !next.same_blocks(previous),
            block_entity_art: false,
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

    /// Compares recorded layer reads; a subscriber without a prior compile must compile.
    pub fn between(
        stack: &ValidatedPackStack,
        previous: Option<(&PackAdmission, &Dependencies)>,
    ) -> Self {
        let prior = previous.and_then(|(admission, _)| match admission {
            PackAdmission::Validated(stack) => Some(stack.as_ref()),
            PackAdmission::None => None,
        });
        // The same archives in the same order replay every recorded read identically.
        let identical = prior.is_some_and(|old| old.same_contents(stack));
        let changed = |subscriber| {
            let Some((old, inputs)) =
                prior.zip(previous.and_then(|(_, dependencies)| dependencies.get(&subscriber)))
            else {
                return true;
            };
            if identical {
                return false;
            }
            let (before, after) = rayon::join(
                || dependency_fingerprint(old, inputs),
                || dependency_fingerprint(stack, inputs),
            );
            before != after
        };
        // Changed blocks already require icon compilation; the other comparisons stay parallel.
        let ((blocks, icons), compared) = rayon::join(
            || {
                let blocks = changed(Subscriber::Blocks);
                (blocks, blocks || changed(Subscriber::Icons))
            },
            || {
                [
                    Subscriber::BlockEntityArt,
                    Subscriber::Atmosphere,
                    Subscriber::Particles,
                    Subscriber::Glyphs,
                    Subscriber::Entities,
                    Subscriber::Ui,
                    Subscriber::Sounds,
                    Subscriber::Language,
                    Subscriber::AimAssist,
                ]
                .into_par_iter()
                .map(|subscriber| (subscriber, changed(subscriber)))
                .collect::<BTreeMap<_, _>>()
            },
        );
        let changed = |subscriber: Subscriber| compared[&subscriber];
        Self {
            contents: true,
            blocks,
            block_entity_art: changed(Subscriber::BlockEntityArt),
            atmosphere: changed(Subscriber::Atmosphere),
            particles: changed(Subscriber::Particles),
            icons,
            glyphs: changed(Subscriber::Glyphs),
            entities: changed(Subscriber::Entities),
            ui: changed(Subscriber::Ui),
            sounds: changed(Subscriber::Sounds),
            language: changed(Subscriber::Language),
            aim_assist: changed(Subscriber::AimAssist),
        }
    }
}

/// Replays readable and rejected file dependencies without consuming unrelated textures.
pub fn dependency_fingerprint(
    stack: &ValidatedPackStack,
    inputs: &BTreeSet<PackDependency>,
) -> [u8; 32] {
    let _zone = tracing::info_span!("pack.diff.fingerprint", inputs = inputs.len()).entered();
    let layers: Vec<_> = stack
        .packs()
        .par_iter()
        .map(|pack| layer_fingerprint(pack, inputs))
        .collect();
    let mut hash = Sha256::new();
    for layer in layers.into_iter().flatten() {
        hash.update(layer);
    }
    hash.finalize().into()
}

/// One input as a layer replays it; uncached reads inflate in parallel.
enum Part<'a> {
    File {
        path: &'a str,
        limit: u64,
    },
    Listed {
        tag: u8,
        prefix: &'a str,
        path: &'a str,
    },
    Contents(&'a str),
}

/// A read's outcome, with the bytes reduced to their digest.
enum Read {
    Bytes([u8; 32]),
    Missing,
    Rejected,
}

/// The layer's digest over `inputs`, or `None` when none of them is present in `pack`.
fn layer_fingerprint(
    pack: &resource_pack::ValidatedPack,
    inputs: &BTreeSet<PackDependency>,
) -> Option<[u8; 32]> {
    let mut parts = Vec::new();
    for input in inputs {
        match input {
            PackDependency::File { path, limit } => parts.push(Part::File {
                path,
                limit: *limit,
            }),
            PackDependency::Directory(prefix) => {
                parts.extend(pack.files_under(prefix).into_vec().into_iter().map(|path| {
                    Part::Listed {
                        tag: 1,
                        prefix,
                        path,
                    }
                }));
            }
            PackDependency::DirectoryWithSuffixes { prefix, suffixes } => parts.extend(
                pack.files_under(prefix)
                    .into_vec()
                    .into_iter()
                    .filter(|path| suffixes.iter().any(|suffix| path.ends_with(suffix)))
                    .map(|path| Part::Listed {
                        tag: 3,
                        prefix,
                        path,
                    }),
            ),
            PackDependency::Contents(prefix) => {
                parts.extend(
                    pack.files_under(prefix)
                        .into_vec()
                        .into_iter()
                        .map(Part::Contents),
                );
            }
        }
    }
    let reads: Vec<_> = parts
        .par_iter()
        .map(|part| match part {
            Part::File { path, limit } => {
                Some(match pack.file_fingerprint_with_limit(path, *limit) {
                    Ok(Some(hash)) => Read::Bytes(hash),
                    Ok(None) => Read::Missing,
                    Err(_) => Read::Rejected,
                })
            }
            Part::Contents(path) => Some(
                match pack.file_fingerprint_with_limit(path, resource_pack::MAX_FILE_BYTES) {
                    Ok(Some(hash)) => Read::Bytes(hash),
                    _ => Read::Missing,
                },
            ),
            Part::Listed { .. } => None,
        })
        .collect();
    let mut layer = Sha256::new();
    let mut populated = false;
    for (part, read) in parts.iter().zip(reads) {
        match (part, read) {
            (Part::File { path, .. }, Some(Read::Bytes(bytes))) => {
                layer.update([0]);
                hash_part(&mut layer, path.as_bytes());
                hash_part(&mut layer, &bytes);
                populated = true;
            }
            (Part::File { path, .. }, Some(Read::Rejected)) => {
                layer.update([4]);
                hash_part(&mut layer, path.as_bytes());
                populated = true;
            }
            (Part::Listed { tag, prefix, path }, _) => {
                layer.update([*tag]);
                hash_part(&mut layer, prefix.as_bytes());
                hash_part(&mut layer, path.as_bytes());
                populated = true;
            }
            (Part::Contents(path), read) => {
                layer.update([2]);
                hash_part(&mut layer, path.as_bytes());
                if let Some(Read::Bytes(bytes)) = read {
                    hash_part(&mut layer, &bytes);
                }
                populated = true;
            }
            (Part::File { .. }, _) => {}
        }
    }
    populated.then(|| layer.finalize().into())
}

/// Length framing prevents two adjacent names or payloads from aliasing one input.
fn hash_part(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

#[cfg(test)]
#[path = "reload_diff/tests.rs"]
mod tests;
