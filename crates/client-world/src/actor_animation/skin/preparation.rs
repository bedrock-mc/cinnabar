//! Immutable skin models shared across actors with identical admitted appearance sources.
use std::{
    collections::{HashMap, VecDeque},
    hash::{BuildHasher, Hash, Hasher},
    sync::{Arc, Mutex, PoisonError},
};

use assets::{RuntimeEntityAssets, SkinGeometry, parse_skin_geometry};
use protocol::SkinGeometrySource;

use super::super::{BoneTransform, RuntimeBone, compose_pose, skeleton, skin_layers};

#[derive(Debug)]
pub(in crate::actor_animation) struct PreparedSkin {
    pub(in crate::actor_animation) geometry: Arc<SkinGeometry>,
    pub(in crate::actor_animation) mesh: Option<render_model::ActorRigGeometry>,
    pub(in crate::actor_animation) bones: Vec<RuntimeBone>,
    pub(in crate::actor_animation) names: Vec<Box<str>>,
    pub(in crate::actor_animation) layers: Vec<skin_layers::SkinLayerSkeleton>,
    pub(in crate::actor_animation) rest: Option<Arc<[BoneTransform]>>,
}

impl PreparedSkin {
    /// Counts retained immutable vertex allocations against the existing actor catalog ceiling.
    pub(in crate::actor_animation) fn mesh_bytes(&self) -> usize {
        self.mesh
            .as_ref()
            .map_or(0, |mesh| std::mem::size_of_val(mesh.vertices.as_ref()))
            + self
                .layers
                .iter()
                .filter_map(|layer| layer.mesh.as_ref())
                .map(|mesh| std::mem::size_of_val(mesh.vertices.as_ref()))
                .sum::<usize>()
    }
}

#[derive(Clone, Debug)]
struct SourceKey {
    source: Arc<SkinGeometrySource>,
    hash: u64,
}

impl PartialEq for SourceKey {
    /// The hash narrows lookup; exact source equality keeps collisions from exchanging appearances.
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.source == other.source
    }
}
impl Eq for SourceKey {}

impl Hash for SourceKey {
    /// Hashes admitted bytes once per changed source, then reuses that hash during cache operations.
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

/// Session-local immutable preparation; each actor retains its own driver map and pose history.
#[derive(Debug, Default)]
pub(in crate::actor_animation) struct SkinPreparationCache {
    entries: HashMap<SourceKey, (Option<Arc<PreparedSkin>>, bool)>,
    order: VecDeque<SourceKey>,
    source_bytes: usize,
    mesh_bytes: usize,
}

impl SkinPreparationCache {
    /// Empty owners need no asynchronous retirement task.
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Shares preparation across overlapping batches; hashing and model construction run unlocked.
    pub(super) fn prepare_shared(
        cache: &Mutex<Self>,
        source: &Arc<SkinGeometrySource>,
        assets: &RuntimeEntityAssets,
    ) -> (Option<Arc<PreparedSkin>>, bool) {
        let lock = || cache.lock().unwrap_or_else(PoisonError::into_inner);
        let hasher = lock().entries.hasher().clone();
        let mut hash = hasher.build_hasher();
        source.resource_patch.hash(&mut hash);
        source.geometry_data.hash(&mut hash);
        source.animations.len().hash(&mut hash);
        for image in source.animations.iter() {
            (
                image.kind.slot(),
                image.width,
                image.height,
                image.frames,
                image.blinking,
            )
                .hash(&mut hash);
            image.rgba8.hash(&mut hash);
        }
        let key = SourceKey {
            source: Arc::clone(source),
            hash: hash.finish(),
        };
        if let Some(prepared) = lock().entries.get(&key) {
            return prepared.clone();
        }
        let prepared = prepare(source, assets);
        lock().insert(key, prepared)
    }

    /// Shares successful and rejected preparation without retaining more source bytes than actor admission.
    fn insert(
        &mut self,
        key: SourceKey,
        prepared: (Option<Arc<PreparedSkin>>, bool),
    ) -> (Option<Arc<PreparedSkin>>, bool) {
        // An overlapping batch may have prepared equal content first; keep sharing its result.
        if let Some(existing) = self.entries.get(&key) {
            return existing.clone();
        }
        let bytes = key.source.byte_len();
        let mesh_bytes = prepared
            .0
            .as_ref()
            .map_or(0, |prepared| prepared.mesh_bytes());
        if bytes > crate::actor_store::MAX_TRACKED_PLAYER_SKIN_BYTES
            || mesh_bytes > render_model::MAX_ACTOR_CATALOG_VERTEX_BYTES
        {
            return prepared;
        }
        while self.entries.len() >= crate::actor_store::MAX_TRACKED_ACTORS
            || self.source_bytes + bytes > crate::actor_store::MAX_TRACKED_PLAYER_SKIN_BYTES
            || self.mesh_bytes + mesh_bytes > render_model::MAX_ACTOR_CATALOG_VERTEX_BYTES
        {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.source_bytes -= oldest.source.byte_len();
            if let Some((Some(prepared), _)) = self.entries.remove(&oldest) {
                self.mesh_bytes -= prepared.mesh_bytes();
            }
        }
        self.source_bytes += bytes;
        self.mesh_bytes += mesh_bytes;
        self.order.push_back(key.clone());
        self.entries.insert(key, prepared.clone());
        prepared
    }
}

/// Sources without model data or animated layers resolve only against the vanilla catalog.
pub(super) fn uses_catalog_model(source: &SkinGeometrySource) -> bool {
    source.animations.is_empty() && matches!(source.geometry_data.trim(), "" | "null")
}

/// Resolves a source against the store's immutable vanilla catalog and composes its rest pose once.
fn prepare(
    source: &SkinGeometrySource,
    assets: &RuntimeEntityAssets,
) -> (Option<Arc<PreparedSkin>>, bool) {
    #[cfg(test)]
    PREPARED_ON_THREAD.with(|count| count.set(count.get() + 1));
    let parsed =
        parse_skin_geometry(&source.resource_patch, &source.geometry_data).map(|geometry| {
            geometry.or_else(|| {
                let name = assets::skin_geometry_name(&source.resource_patch)?;
                let geometry = assets
                    .geometries()
                    .iter()
                    .find(|geometry| geometry.identifier.eq_ignore_ascii_case(&name))?;
                SkinGeometry::from_catalog(geometry)
            })
        });
    match parsed {
        Ok(Some(geometry)) => {
            let Some((bones, names)) = skeleton(&geometry.bones) else {
                return (None, true);
            };
            let rest = compose_pose(&bones, &[]).map(Arc::from);
            (
                Some(Arc::new(PreparedSkin {
                    mesh: render_model::skin_geometry(&geometry, render_model::DIAGNOSTIC_RIG_ID)
                        .ok(),
                    geometry: Arc::new(geometry),
                    bones,
                    layers: skin_layers::parse(source, &names),
                    names,
                    rest,
                })),
                false,
            )
        }
        Ok(None) => (None, false),
        Err(_) => (None, true),
    }
}

#[cfg(test)]
thread_local! {
    /// Preparations run on the calling thread, so tests can prove the frame thread does none.
    pub(super) static PREPARED_ON_THREAD: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
