//! Nonblocking, session-owned preparation of immutable player appearances.
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

use assets::RuntimeEntityAssets;
use render_api::SkinGeometrySource;

use super::preparation::{PreparedSkin, SkinPreparationCache as WorkerCache};

/// A publication pass admits this many cold sources to the worker.
pub(crate) const MAX_SKIN_PREPARATIONS_PER_PASS: usize = 8;
/// Batches running at once; later passes keep their requests queued until one completes.
pub(super) const MAX_SKIN_BATCHES_IN_FLIGHT: usize = 4;
/// Model-free sources at or below this size share one worker-prepared result per catalog model.
const MAX_CATALOG_SOURCE_BYTES: usize = 1024;
const MAX_CATALOG_MODELS: usize = 32;
/// Classic and slim skins name these; preparing them with the session makes first sight inline.
const STANDARD_CATALOG_MODELS: [&str; 2] =
    ["geometry.humanoid.custom", "geometry.humanoid.customSlim"];
/// The catalog model a model-free source names; `None` inside means the default model applies.
type CatalogKey = Option<Box<str>>;
// A pending replacement may coexist with one fully ready appearance per admitted player.
const MAX_SOURCES: usize = crate::actor_store::MAX_TRACKED_ACTORS * 2;
const SOURCE_BYTES: usize = crate::actor_store::MAX_TRACKED_PLAYER_SKIN_BYTES * 2;
const MESH_BYTES: usize = render_model::MAX_ACTOR_CATALOG_VERTEX_BYTES * 2;
type Outcome = (Option<Arc<PreparedSkin>>, bool);

#[derive(Debug)]
struct Entry {
    source: Arc<SkinGeometrySource>,
    outcome: Option<Outcome>,
    seen: u64,
    allocation: Option<usize>,
    in_flight_references: usize,
    unchanged_from: Option<Weak<SkinGeometrySource>>,
}

#[derive(Debug)]
struct Allocation {
    _prepared: Arc<PreparedSkin>,
    references: usize,
    bytes: usize,
}

#[derive(Debug)]
struct Request {
    source: Arc<SkinGeometrySource>,
    previous: Option<(Arc<SkinGeometrySource>, Outcome)>,
}

#[derive(Debug)]
struct Completion {
    source: Arc<SkinGeometrySource>,
    catalog: Option<CatalogKey>,
    outcome: Outcome,
    previous: Option<Arc<SkinGeometrySource>>,
    unchanged: bool,
}

#[derive(Debug)]
struct Completed {
    sources: Vec<Completion>,
}

/// Main-thread operations inspect only retained pointers and bounded completion batches.
#[derive(Debug)]
pub(in crate::actor_animation) struct SkinPreparationQueue {
    entries: HashMap<usize, Entry>,
    allocations: HashMap<usize, Allocation>,
    mesh_budget: usize,
    queued: Vec<Request>,
    cache: Arc<Mutex<WorkerCache>>,
    catalog: HashMap<CatalogKey, Outcome>,
    sender: mpsc::Sender<Completed>,
    receiver: Mutex<mpsc::Receiver<Completed>>,
    in_flight: usize,
    cancelled: Arc<AtomicBool>,
    frame: u64,
    source_bytes: usize,
    mesh_bytes: usize,
}

impl Default for SkinPreparationQueue {
    /// Each owner has a separate completion channel and cache lifetime.
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            entries: HashMap::new(),
            allocations: HashMap::new(),
            mesh_budget: MESH_BYTES,
            queued: Vec::new(),
            cache: Arc::default(),
            catalog: HashMap::new(),
            sender,
            receiver: Mutex::new(receiver),
            in_flight: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
            frame: 0,
            source_bytes: 0,
            mesh_bytes: 0,
        }
    }
}

impl Drop for SkinPreparationQueue {
    /// An obsolete worker can finish its current source, but cannot publish into another owner.
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let entries = std::mem::take(&mut self.entries);
        let allocations = std::mem::take(&mut self.allocations);
        let cache = std::mem::take(&mut self.cache);
        let catalog = std::mem::take(&mut self.catalog);
        let queued = std::mem::take(&mut self.queued);
        let in_flight = self.in_flight > 0;
        if !entries.is_empty()
            || !queued.is_empty()
            || !catalog.is_empty()
            || in_flight
            || !cache.try_lock().is_ok_and(|cache| cache.is_empty())
        {
            let (_, idle) = mpsc::channel();
            let receiver = std::mem::replace(&mut self.receiver, Mutex::new(idle));
            rayon::spawn(move || {
                let _span = tracing::info_span!("actor.skin_retire").entered();
                drop((entries, allocations, cache, catalog, queued, receiver));
            });
        }
    }
}

impl SkinPreparationQueue {
    pub(in crate::actor_animation) fn is_pending(&self) -> bool {
        self.in_flight > 0 || !self.queued.is_empty()
    }

    /// Drains every finished bounded batch and retires pointers absent from the last complete pass.
    pub(in crate::actor_animation) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        while self.in_flight > 0 {
            let Ok(completed) = self
                .receiver
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .try_recv()
            else {
                break;
            };
            self.complete(completed);
        }
        let oldest = self.frame.saturating_sub(2);
        let mut retired = Vec::new();
        self.entries.retain(|_, entry| {
            if entry.seen >= oldest || entry.outcome.is_none() || entry.in_flight_references > 0 {
                return true;
            }
            self.source_bytes -= entry.source.byte_len();
            if let Some(id) = entry.allocation {
                let allocation = self
                    .allocations
                    .get_mut(&id)
                    .expect("retained allocation charge");
                allocation.references -= 1;
                if allocation.references == 0 {
                    self.mesh_bytes -= allocation.bytes;
                    self.allocations.remove(&id);
                }
            }
            retired.push((Arc::clone(&entry.source), entry.outcome.take()));
            false
        });
        if !retired.is_empty() {
            rayon::spawn(move || {
                let _span = tracing::info_span!("actor.skin_retire").entered();
                drop(retired);
            });
        }
    }

    /// Looks up a ready source or queues its retained allocation without hashing its contents; pins
    /// the prior result so an equal replacement can preserve animation history after memo eviction.
    pub(in crate::actor_animation) fn request_replacing(
        &mut self,
        source: &Arc<SkinGeometrySource>,
        previous: Option<&Arc<SkinGeometrySource>>,
    ) -> bool {
        let pointer = Arc::as_ptr(source) as usize;
        if let Some(entry) = self.entries.get_mut(&pointer) {
            entry.seen = self.frame;
            return entry.outcome.is_some();
        }
        if self.entries.len() >= MAX_SOURCES
            || self.source_bytes.saturating_add(source.byte_len()) > SOURCE_BYTES
        {
            return false;
        }
        // Only an already prepared catalog model resolves here; cold ones take the worker path.
        if let Some(outcome) = catalog_key(source).and_then(|key| self.catalog.get(&key).cloned()) {
            self.admit(source);
            // The catalog memo still owns any over-budget result, so nothing large drops here.
            let _ = self.settle(Arc::clone(source), outcome, None, false);
            return true;
        }
        if self.queued.len() >= MAX_SKIN_PREPARATIONS_PER_PASS {
            return false;
        }
        let previous = previous.and_then(|source| {
            let entry = self.entries.get_mut(&(Arc::as_ptr(source) as usize))?;
            let outcome = entry.outcome.clone()?;
            entry.in_flight_references += 1;
            Some((Arc::clone(source), outcome))
        });
        self.admit(source);
        self.queued.push(Request {
            source: Arc::clone(source),
            previous,
        });
        false
    }

    /// Charges a new pending entry against the retained source budget.
    fn admit(&mut self, source: &Arc<SkinGeometrySource>) {
        self.source_bytes += source.byte_len();
        self.entries.insert(
            Arc::as_ptr(source) as usize,
            Entry {
                source: Arc::clone(source),
                outcome: None,
                seen: self.frame,
                allocation: None,
                in_flight_references: 0,
                unchanged_from: None,
            },
        );
    }

    /// Starts preparing the standard catalog models on the worker before any player names them.
    pub(in crate::actor_animation) fn prewarm_catalog(
        &mut self,
        assets: &Arc<RuntimeEntityAssets>,
    ) {
        for name in STANDARD_CATALOG_MODELS {
            self.queued.push(Request {
                source: Arc::new(SkinGeometrySource {
                    resource_patch: format!(r#"{{"geometry":{{"default":"{name}"}}}}"#).into(),
                    geometry_data: "".into(),
                    animations: Arc::from([]),
                }),
                previous: None,
            });
        }
        self.submit(assets);
    }

    /// A completed source is immutable for the entire owner lifetime.
    pub(in crate::actor_animation) fn get(
        &self,
        source: &Arc<SkinGeometrySource>,
    ) -> Option<Outcome> {
        self.entries
            .get(&(Arc::as_ptr(source) as usize))?
            .outcome
            .clone()
    }

    /// The weak allocation identity cannot be recycled while any replacement still refers to it.
    pub(in crate::actor_animation) fn replaces_unchanged(
        &self,
        source: &Arc<SkinGeometrySource>,
        previous: &Arc<SkinGeometrySource>,
    ) -> bool {
        self.entries
            .get(&(Arc::as_ptr(source) as usize))
            .and_then(|entry| entry.unchanged_from.as_ref())
            .is_some_and(|identity| identity.as_ptr() == Arc::as_ptr(previous))
    }

    /// Moves hashing, exact content comparison, parsing and mesh construction onto Rayon.
    pub(in crate::actor_animation) fn submit(&mut self, assets: &Arc<RuntimeEntityAssets>) {
        if self.queued.is_empty() || self.in_flight >= MAX_SKIN_BATCHES_IN_FLIGHT {
            return;
        }
        self.in_flight += 1;
        let queued = std::mem::take(&mut self.queued);
        let assets = Arc::clone(assets);
        let cache = Arc::clone(&self.cache);
        let cancelled = Arc::clone(&self.cancelled);
        let send = self.sender.clone();
        rayon::spawn(move || {
            let _batch =
                tracing::info_span!("actor.skin_prepare_batch", sources = queued.len()).entered();
            let mut sources = Vec::with_capacity(queued.len());
            for Request { source, previous } in queued {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                let _source =
                    tracing::info_span!("actor.skin_prepare", bytes = source.byte_len()).entered();
                let unchanged = previous
                    .as_ref()
                    .is_some_and(|(prior, _)| prior.as_ref() == source.as_ref());
                let outcome = if unchanged {
                    previous
                        .as_ref()
                        .expect("unchanged source has a prior result")
                        .1
                        .clone()
                } else {
                    WorkerCache::prepare_shared(&cache, &source, &assets)
                };
                sources.push(Completion {
                    catalog: catalog_key(&source),
                    source,
                    outcome,
                    previous: previous.map(|(source, _)| source),
                    unchanged,
                });
            }
            let _ = send.send(Completed { sources });
        });
    }

    /// Checks request identity and the retained mesh budget before making any result visible.
    fn complete(&mut self, completed: Completed) {
        let _span = tracing::info_span!("actor.skin_completion", sources = completed.sources.len())
            .entered();
        self.in_flight -= 1;
        let mut retired = Vec::new();
        for Completion {
            source,
            catalog,
            outcome,
            previous,
            unchanged,
        } in completed.sources
        {
            if let Some(key) = catalog
                && self.catalog.len() < MAX_CATALOG_MODELS
            {
                self.catalog.entry(key).or_insert_with(|| outcome.clone());
            }
            if let Some(previous) = &previous
                && let Some(entry) = self.entries.get_mut(&(Arc::as_ptr(previous) as usize))
            {
                entry.in_flight_references -= 1;
            }
            retired.extend(self.settle(source, outcome, previous, unchanged));
        }
        if !retired.is_empty() {
            rayon::spawn(move || {
                let _span = tracing::info_span!("actor.skin_retire").entered();
                drop(retired);
            });
        }
    }

    /// Makes one result visible within the retained mesh budget; returns a rejected result to retire.
    fn settle(
        &mut self,
        source: Arc<SkinGeometrySource>,
        outcome: Outcome,
        previous: Option<Arc<SkinGeometrySource>>,
        unchanged: bool,
    ) -> Option<(Arc<SkinGeometrySource>, Outcome)> {
        let pointer = Arc::as_ptr(&source) as usize;
        let entry = self.entries.get_mut(&pointer)?;
        if !Arc::ptr_eq(&entry.source, &source) || entry.outcome.is_some() {
            return None;
        }
        let allocation = outcome
            .0
            .as_ref()
            .map(|prepared| Arc::as_ptr(prepared) as usize);
        let bytes = outcome
            .0
            .as_ref()
            .filter(|prepared| {
                !self
                    .allocations
                    .contains_key(&(Arc::as_ptr(prepared) as usize))
            })
            .map_or(0, |prepared| prepared.mesh_bytes());
        if self.mesh_bytes.saturating_add(bytes) > self.mesh_budget {
            entry.outcome = Some((None, true));
            return Some((source, outcome));
        }
        self.mesh_bytes += bytes;
        if let (Some(id), Some(prepared)) = (allocation, &outcome.0) {
            self.allocations
                .entry(id)
                .or_insert_with(|| Allocation {
                    _prepared: Arc::clone(prepared),
                    references: 0,
                    bytes,
                })
                .references += 1;
        }
        entry.allocation = allocation;
        entry.unchanged_from = previous.filter(|_| unchanged).as_ref().map(Arc::downgrade);
        entry.outcome = Some(outcome);
        None
    }

    /// Explicit test synchronization observes completed work without asserting elapsed time.
    #[cfg(test)]
    pub(in crate::actor_animation) fn finish_fixture_batch(&mut self) {
        while self.in_flight > 0 {
            let completed = self.receiver.get_mut().unwrap().recv().unwrap();
            self.complete(completed);
        }
    }
}

/// Model-free sources resolve purely by the catalog model their patch names.
fn catalog_key(source: &SkinGeometrySource) -> Option<CatalogKey> {
    (source.byte_len() <= MAX_CATALOG_SOURCE_BYTES
        && super::preparation::uses_catalog_model(source))
    .then(|| assets::skin_geometry_name(&source.resource_patch).map(Into::into))
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
