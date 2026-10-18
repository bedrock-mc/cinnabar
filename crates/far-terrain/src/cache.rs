//! One client's far-terrain cache: the active namespace in memory, persisted per region.
//!
//! Every summary carries a version stamped when it was taken, from a counter that only rises
//! across runs. Memory, waiting writes, writes in flight, region files and disk reads all keep
//! the higher version of a column, so an older summary never replaces a newer one wherever and
//! however late it arrives.

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
use web_time::Instant;

use crate::{
    ColumnPos, ColumnSummary, EncodedSummary, FarTerrainConfig, NamespaceKey, ServerKey,
    SummaryBudget, SummaryStore, Versioned, WorldKey,
    config::{
        MAX_UNRESOLVED_COLUMNS, REGION_FLUSH_AFTER, REGION_FLUSH_COLUMNS, WORLD_DIVERGED_COLUMNS,
        WORLD_PROBE_COLUMNS, WORLD_PROBE_SIMILARITY, WORLD_PROBE_TIMEOUT, WORLD_VERIFY_COLUMNS,
    },
    disk::{DiskBounds, DiskCache},
    identity::surface_similarity,
    region::{REGION_SIDE, RegionPos, region_index},
};

mod completion;
use completion::PendingEvent;

#[derive(Debug)]
struct DirtyRegion {
    since: Instant,
    /// Payloads are kept here, not looked up at flush, so memory eviction cannot lose them.
    columns: BTreeMap<ColumnPos, Versioned>,
}

/// A world just left, which late summaries still reach.
enum Retired {
    Resolved(NamespaceKey),
    /// Left before its namespace resolved; late columns are written by resolving with its
    /// probes on the IO thread.
    Unresolved {
        world: WorldKey,
        /// Its visit, so every late batch resolves to the same epoch.
        entry: u64,
        probes: Vec<(ColumnPos, ColumnSummary)>,
        columns: Vec<(ColumnPos, Versioned)>,
    },
}

impl Retired {
    const fn dimension(&self) -> i32 {
        match self {
            Self::Resolved(namespace) => namespace.dimension,
            Self::Unresolved { world, .. } => world.dimension,
        }
    }
}

/// A session whose final summaries are still being extracted, from
/// [`FarTerrainCache::retire_session`]; hand it back to [`FarTerrainCache::persist`]. Its
/// summaries carry the version stamped at retirement, so anything taken later wins.
#[derive(Clone, Debug)]
#[must_use]
pub struct RetiringSession {
    world: WorldKey,
    /// The visit retiring, whose namespace its summaries share once resolved.
    entry: u64,
    namespace: Option<NamespaceKey>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    version: u64,
}

impl RetiringSession {
    /// Keeps the first non-empty retirement probes before persistence is split into batches.
    pub fn add_probe(&mut self, pos: ColumnPos, summary: &EncodedSummary) {
        if self.probes.len() < WORLD_PROBE_COLUMNS
            && let Ok(decoded) = ColumnSummary::decode(&summary.bytes)
            && !decoded.is_empty()
        {
            self.probes.push((pos, decoded));
        }
    }

    #[must_use]
    pub const fn dimension(&self) -> i32 {
        self.world.dimension
    }
}

/// Summaries of the world the client is in, with reads and batched writes on an IO thread.
///
/// A world's namespace is resolved from its first non-empty columns, so a server that reuses
/// one level identity for different maps gets a fresh namespace instead of mixed terrain.
pub struct FarTerrainCache {
    /// Started on the first world entered, so a client that never uses far terrain runs no
    /// IO thread.
    disk: Option<DiskCache>,
    disk_root: Option<(PathBuf, DiskBounds)>,
    store: SummaryStore,
    world: Option<WorldKey>,
    namespace: Option<NamespaceKey>,
    /// The world left by the last world change; late summaries for it go straight to disk.
    retired: Option<Retired>,
    /// Columns shown before the namespace resolved, written once it does.
    unresolved: Vec<(ColumnPos, Versioned)>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    resolving: bool,
    /// When this visit's first column arrived, for [`WORLD_PROBE_TIMEOUT`].
    pending_since: Option<Instant>,
    /// Changes waiting to be written, per namespace and region.
    dirty: BTreeMap<(NamespaceKey, RegionPos), DirtyRegion>,
    /// Changes handed to the IO thread and not yet reported written.
    writing: BTreeMap<(NamespaceKey, RegionPos), BTreeMap<ColumnPos, Versioned>>,
    pending_event: Option<PendingEvent>,
    requested: HashSet<RegionPos>,
    /// Regions read again after memory dropped some of their columns, once per focus.
    reread: HashSet<RegionPos>,
    focus: Option<(ColumnPos, i32)>,
    /// Bumped on every world change, so a resolution for an earlier visit is ignored.
    entry: u64,
    next_version: u64,
    /// First version stamped this visit; anything older in memory came from disk.
    visit_start: u64,
    /// Cached columns compared with live ones this visit, and how many differed.
    compared: u32,
    mismatched: u32,
}

impl FarTerrainCache {
    /// A cache persisted under `disk_root`, or memory-only without one. `version_base` must
    /// exceed every version an earlier run stamped, as a clock reading in microseconds does.
    #[must_use]
    pub fn new(config: FarTerrainConfig, disk_root: Option<PathBuf>, version_base: u64) -> Self {
        let bounds = DiskBounds {
            server_cap_bytes: config.server_disk_cap_bytes,
            global_cap_bytes: config.global_disk_cap_bytes,
        };
        Self {
            disk: None,
            disk_root: disk_root.map(|root| (root, bounds)),
            store: SummaryStore::new(config.memory_budget_bytes),
            world: None,
            namespace: None,
            retired: None,
            unresolved: Vec::new(),
            probes: Vec::new(),
            resolving: false,
            pending_since: None,
            dirty: BTreeMap::new(),
            writing: BTreeMap::new(),
            pending_event: None,
            requested: HashSet::new(),
            reread: HashSet::new(),
            focus: None,
            entry: 0,
            next_version: version_base,
            visit_start: version_base,
            compared: 0,
            mismatched: 0,
        }
    }

    /// Switches to `dimension` of the level `level` on the server at `destination`.
    /// Anything else, including a transfer, flushes and leaves the previous namespace.
    pub fn enter_world(&mut self, destination: &str, level: u64, dimension: i32) {
        let world = WorldKey {
            server: ServerKey::from_destination(destination),
            dimension,
            level,
            resources: [0; 32],
        };
        if self.world != Some(world) {
            self.begin_visit(destination, level, dimension);
        }
    }

    /// Starts a fresh visit to `dimension` of `level` on `destination`, even of the world
    /// already entered, as after a transfer: nothing from the previous visit stays shown,
    /// and its namespace is resolved again from new probes.
    pub fn begin_visit(&mut self, destination: &str, level: u64, dimension: i32) {
        self.begin_visit_with_resources(destination, level, dimension, [0; 32]);
    }

    /// Starts a visit whose disk and memory summaries belong to these resource inputs.
    pub fn begin_visit_with_resources(
        &mut self,
        destination: &str,
        level: u64,
        dimension: i32,
        resources: [u8; 32],
    ) {
        self.end_entry();
        if let Some((root, bounds)) = self.disk_root.take() {
            self.disk = DiskCache::spawn(root, bounds);
        }
        self.world = Some(WorldKey {
            server: ServerKey::from_destination(destination),
            dimension,
            level,
            resources,
        });
    }

    /// Starts a visit without a stable world identity. Its empty probe set reserves a new
    /// epoch before live summaries arrive, so matching lobby columns cannot enable reuse.
    pub fn begin_uncertain_visit(&mut self, destination: &str, dimension: i32) {
        self.begin_uncertain_visit_with_resources(destination, dimension, [0; 32]);
    }

    /// Reserves a fresh world epoch while keeping other resource generations separate.
    pub fn begin_uncertain_visit_with_resources(
        &mut self,
        destination: &str,
        dimension: i32,
        resources: [u8; 32],
    ) {
        self.begin_visit_with_resources(destination, 0, dimension, resources);
        self.start_resolving();
    }

    /// Leaves the current world, as on disconnect, after writing what is waiting.
    pub fn leave_world(&mut self) {
        self.end_entry();
        self.world = None;
    }

    /// Writes everything the visit holds, resolving its namespace on the IO thread with the
    /// probes it has when it never resolved.
    fn end_entry(&mut self) {
        if let Some(namespace) = self.namespace {
            for (pos, column) in std::mem::take(&mut self.unresolved) {
                self.queue_write(
                    namespace,
                    pos,
                    column,
                    self.pending_since
                        .expect("unresolved columns have an admission time"),
                );
            }
        }
        self.flush_all();
        self.flush_retired();
        if self.namespace.is_none()
            && let (Some(world), Some(disk)) = (self.world, &self.disk)
            && !self.unresolved.is_empty()
        {
            disk.resolve_and_write(
                world,
                self.entry,
                self.probes.clone(),
                std::mem::take(&mut self.unresolved),
            );
        }
        self.retired = match (self.namespace.take(), self.world) {
            (Some(namespace), _) => Some(Retired::Resolved(namespace)),
            (None, Some(world)) => Some(Retired::Unresolved {
                world,
                entry: self.entry,
                probes: std::mem::take(&mut self.probes),
                columns: Vec::new(),
            }),
            (None, None) => None,
        };
        self.entry += 1;
        self.visit_start = self.next_version + 1;
        self.compared = 0;
        self.mismatched = 0;
        self.store.clear();
        self.unresolved.clear();
        self.probes.clear();
        self.resolving = false;
        self.pending_since = None;
        self.requested.clear();
        self.reread.clear();
    }

    /// Writes late columns of a world left before its namespace resolved.
    fn flush_retired(&mut self) {
        if let (
            Some(Retired::Unresolved {
                world,
                entry,
                probes,
                columns,
            }),
            Some(disk),
        ) = (&mut self.retired, &self.disk)
            && !columns.is_empty()
        {
            // Late columns probe too, so a world left before its first summary is still
            // compared against cached maps rather than reusing the newest blindly.
            add_probes(probes, columns);
            disk.resolve_and_write(*world, *entry, probes.clone(), std::mem::take(columns));
        }
    }

    /// Records a column's encoded summary as the newest of it. Columns show at once and are
    /// written once the world's namespace is resolved.
    pub fn admit(&mut self, dimension: i32, pos: ColumnPos, summary: EncodedSummary, now: Instant) {
        let column = Versioned {
            version: self.stamp(),
            summary,
        };
        if self
            .retired
            .as_ref()
            .is_some_and(|retired| retired.dimension() == dimension)
            && self.world.is_none_or(|world| world.dimension != dimension)
        {
            match &mut self.retired {
                Some(Retired::Resolved(namespace)) => {
                    let namespace = *namespace;
                    self.queue_write(namespace, pos, column, now);
                }
                Some(Retired::Unresolved { columns, .. })
                    if self.disk.is_some() && columns.len() < MAX_UNRESOLVED_COLUMNS =>
                {
                    columns.push((pos, column));
                }
                Some(Retired::Unresolved { .. }) | None => {}
            }
            return;
        }
        if self.world.is_none_or(|world| world.dimension != dimension) {
            return;
        }
        if let Some(namespace) = self.namespace {
            if self.verify(pos, &column) {
                self.diverge();
                self.admit(dimension, pos, column.summary, now);
                return;
            }
            self.store.admit_versioned(pos, column.clone());
            self.queue_write(namespace, pos, column, now);
            return;
        }
        self.pending_since.get_or_insert(now);
        if !self.resolving
            && self.probes.len() < WORLD_PROBE_COLUMNS
            && let Ok(decoded) = ColumnSummary::decode(&column.summary.bytes)
            && !decoded.is_empty()
        {
            self.probes.push((pos, decoded));
        }
        if self.disk.is_some() && self.unresolved.len() < MAX_UNRESOLVED_COLUMNS {
            self.unresolved.push((pos, column.clone()));
        }
        self.store.admit_versioned(pos, column);
        if !self.resolving && self.probes.len() == WORLD_PROBE_COLUMNS {
            self.start_resolving();
        }
    }

    /// Loads cached regions within `radius` chunks of `center` once each.
    pub fn set_focus(&mut self, center: ColumnPos, radius: i32) {
        if self.focus == Some((center, radius)) {
            return;
        }
        self.focus = Some((center, radius));
        self.reread.clear();
        self.request_focus();
    }

    /// Applies finished disk work, resolves a sparse world after [`WORLD_PROBE_TIMEOUT`] and
    /// writes regions whose changes are due.
    pub fn poll(&mut self, now: Instant) {
        self.poll_budgeted(now, &mut SummaryBudget::default());
    }

    /// Applies a bounded share of completed disk columns, retaining partial events in order.
    pub fn poll_budgeted(&mut self, now: Instant, budget: &mut SummaryBudget) {
        self.apply_completions(now, budget);
        if self.namespace.is_none()
            && !self.resolving
            && self
                .pending_since
                .is_some_and(|since| now.saturating_duration_since(since) >= WORLD_PROBE_TIMEOUT)
        {
            self.start_resolving();
        }
        self.flush_retired();
        let evicted = self.store.take_evicted();
        self.reread_evicted(&evicted);
        let due: Vec<(NamespaceKey, RegionPos)> = self
            .dirty
            .iter()
            .filter(|(_, dirty)| {
                dirty.columns.len() >= REGION_FLUSH_COLUMNS
                    || now.saturating_duration_since(dirty.since) >= REGION_FLUSH_AFTER
            })
            .map(|(region, _)| *region)
            .collect();
        for region in due {
            self.flush(region);
        }
    }

    /// Writes every waiting change now.
    pub fn flush_all(&mut self) {
        let regions: Vec<(NamespaceKey, RegionPos)> = self.dirty.keys().copied().collect();
        for region in regions {
            self.flush(region);
        }
    }

    /// Starts a session's retirement while its final summaries are extracted, stamping the
    /// version they will carry; `None` outside a world.
    pub fn retire_session(&mut self) -> Option<RetiringSession> {
        let world = self.world?;
        Some(RetiringSession {
            world,
            entry: self.entry,
            namespace: self.namespace,
            probes: self.probes.clone(),
            version: self.stamp(),
        })
    }

    /// Writes a retired session's final summaries, and shows them when their namespace is
    /// active; wherever a newer version exists it is kept.
    pub fn persist(
        &mut self,
        session: RetiringSession,
        columns: impl IntoIterator<Item = (ColumnPos, EncodedSummary)>,
        now: Instant,
    ) {
        // Memory-only visits cannot establish a shared namespace with an earlier map.
        if self.disk.is_none() && session.entry != self.entry {
            return;
        }
        let RetiringSession {
            world,
            entry,
            namespace,
            mut probes,
            version,
        } = session;
        let mut unresolved = Vec::new();
        for (pos, summary) in columns {
            let column = Versioned { version, summary };
            if namespace.is_some_and(|namespace| self.newer_queued(namespace, pos, version)) {
                continue;
            }
            if namespace.is_some() && namespace == self.namespace {
                self.store.admit_versioned(pos, column.clone());
            }
            match namespace {
                Some(namespace) => self.queue_write(namespace, pos, column, now),
                None => unresolved.push((pos, column)),
            }
        }
        if let Some(disk) = &self.disk
            && !unresolved.is_empty()
        {
            add_probes(&mut probes, &unresolved);
            disk.resolve_and_write(world, entry, probes, unresolved);
        }
    }

    /// Resizes the in-memory summary budget, as when the horizon grows or shrinks.
    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.store.set_budget(bytes);
    }

    /// Waits until the IO thread has finished every queued command.
    pub fn sync(&self) {
        if let Some(disk) = &self.disk {
            disk.sync();
        }
    }

    /// The active namespace, once its world identity is resolved.
    #[must_use]
    pub const fn namespace(&self) -> Option<NamespaceKey> {
        self.namespace
    }

    /// Summaries of the current world, including those shown before its namespace resolves.
    pub fn store(&mut self) -> &mut SummaryStore {
        &mut self.store
    }

    /// True when a version of `pos` at least `version` waits to be written or is being written.
    fn newer_queued(&self, namespace: NamespaceKey, pos: ColumnPos, version: u64) -> bool {
        self.queued(namespace, pos)
            .is_some_and(|queued| queued.version >= version)
    }

    /// The newest pending write for a column, whether waiting or already on the IO thread.
    fn queued(&self, namespace: NamespaceKey, pos: ColumnPos) -> Option<&Versioned> {
        let key = (namespace, RegionPos::of(pos));
        self.dirty
            .get(&key)
            .and_then(|dirty| dirty.columns.get(&pos))
            .into_iter()
            .chain(self.writing.get(&key).and_then(|columns| columns.get(&pos)))
            .max_by_key(|column| column.version)
    }

    /// Compares a live column with the cached one it replaces, for the first
    /// [`WORLD_VERIFY_COLUMNS`] of a visit; true once enough differ that the cache must hold
    /// another map under this identity.
    fn verify(&mut self, pos: ColumnPos, live: &Versioned) -> bool {
        if self.compared >= WORLD_VERIFY_COLUMNS
            || self
                .store
                .version(pos)
                .is_none_or(|version| version >= self.visit_start)
        {
            return false;
        }
        let (Some(Ok(cached)), Ok(live)) = (
            self.store
                .encoded(pos)
                .map(|cached| ColumnSummary::decode(&cached.bytes)),
            ColumnSummary::decode(&live.summary.bytes),
        ) else {
            return false;
        };
        self.compared += 1;
        if surface_similarity(&cached, &live) < WORLD_PROBE_SIMILARITY {
            self.mismatched += 1;
        }
        self.mismatched >= WORLD_DIVERGED_COLUMNS && self.mismatched * 4 >= self.compared
    }

    /// Drops what the cache showed for this visit and starts a new epoch for it, keeping
    /// every live column, which is written there once it resolves.
    fn diverge(&mut self) {
        let (Some(world), Some(namespace), Some(disk)) =
            (self.world, self.namespace.take(), &self.disk)
        else {
            return;
        };
        self.store.retain_since(self.visit_start);
        let keys: Vec<(NamespaceKey, RegionPos)> = self
            .dirty
            .keys()
            .copied()
            .filter(|(written, _)| *written == namespace)
            .collect();
        for key in keys {
            let Some(dirty) = self.dirty.get_mut(&key) else {
                continue;
            };
            let visit = self.visit_start;
            let (live, older): (BTreeMap<_, _>, BTreeMap<_, _>) =
                std::mem::take(&mut dirty.columns)
                    .into_iter()
                    .partition(|(_, column)| column.version >= visit);
            dirty.columns = older;
            if dirty.columns.is_empty() {
                self.dirty.remove(&key);
            }
            // Every one moves, uncapped: they are this visit's terrain.
            self.unresolved.extend(live);
        }
        self.entry += 1;
        self.resolving = true;
        self.requested.clear();
        self.reread.clear();
        self.compared = WORLD_VERIFY_COLUMNS;
        // Columns of this visit already written, or in flight before this, move on the IO
        // thread, which handles commands in order.
        disk.resolve_fresh(world, self.entry, namespace, self.visit_start);
    }

    fn stamp(&mut self) -> u64 {
        if let Some(disk) = &self.disk {
            self.next_version = self.next_version.max(disk.observed_version());
        }
        self.next_version += 1;
        self.next_version
    }

    fn start_resolving(&mut self) {
        let Some(world) = self.world else {
            return;
        };
        self.resolving = true;
        match &self.disk {
            Some(disk) => disk.resolve(world, self.entry, self.probes.clone()),
            None => self.resolved(world, 0),
        }
    }

    fn resolved(&mut self, world: WorldKey, epoch: u32) {
        if self.world != Some(world) || self.namespace.is_some() {
            return;
        }
        let namespace = world.namespace(epoch);
        self.namespace = Some(namespace);
        if self.disk.is_none() {
            self.unresolved.clear();
        }
        self.request_focus();
    }

    /// Queues `column` for writing unless a newer version of it already waits.
    fn queue_write(
        &mut self,
        namespace: NamespaceKey,
        pos: ColumnPos,
        column: Versioned,
        now: Instant,
    ) {
        if self.disk.is_none() {
            return;
        }
        let waiting = &mut self
            .dirty
            .entry((namespace, RegionPos::of(pos)))
            .or_insert_with(|| DirtyRegion {
                since: now,
                columns: BTreeMap::new(),
            })
            .columns;
        if waiting
            .get(&pos)
            .is_none_or(|queued| queued.version < column.version)
        {
            waiting.insert(pos, column);
        }
    }

    fn flush(&mut self, key: (NamespaceKey, RegionPos)) {
        let (Some(dirty), Some(disk)) = (self.dirty.remove(&key), &self.disk) else {
            return;
        };
        let mut columns = Vec::with_capacity(dirty.columns.len());
        for (pos, column) in dirty.columns {
            let in_flight = self
                .writing
                .entry(key)
                .or_default()
                .entry(pos)
                .or_insert_with(|| column.clone());
            if in_flight.version < column.version {
                *in_flight = column.clone();
            }
            columns.push((region_index(pos), column));
        }
        disk.write(key.0, key.1, columns);
    }

    /// Reads regions again whose columns memory dropped while still in focus, once per
    /// focus, so a budget shortfall cannot turn into a read loop.
    fn reread_evicted(&mut self, evicted: &[ColumnPos]) {
        let Some((center, radius)) = self.focus else {
            return;
        };
        let mut forgot = false;
        for pos in evicted {
            let region = RegionPos::of(*pos);
            if (pos.x - center.x).abs() <= radius
                && (pos.z - center.z).abs() <= radius
                && self.reread.insert(region)
            {
                forgot |= self.requested.remove(&region);
            }
        }
        if forgot {
            self.request_focus();
        }
    }

    fn request_focus(&mut self) {
        let (Some(disk), Some(namespace), Some((center, radius))) =
            (&self.disk, self.namespace, self.focus)
        else {
            return;
        };
        let low = RegionPos::of(ColumnPos::new(center.x - radius, center.z - radius));
        let high = RegionPos::of(ColumnPos::new(center.x + radius, center.z + radius));
        // Forget regions well outside the focus so returning reloads them.
        let keep = radius / REGION_SIDE + 2;
        let centre = RegionPos::of(center);
        self.requested.retain(|region| {
            (region.x - centre.x).abs() <= keep && (region.z - centre.z).abs() <= keep
        });
        for x in low.x..=high.x {
            for z in low.z..=high.z {
                let region = RegionPos { x, z };
                if self.requested.insert(region) {
                    disk.load(namespace, region);
                }
            }
        }
    }
}

impl Drop for FarTerrainCache {
    fn drop(&mut self) {
        self.end_entry();
    }
}

/// Tops `probes` up to [`WORLD_PROBE_COLUMNS`] with the non-empty ones among `columns`.
fn add_probes(probes: &mut Vec<(ColumnPos, ColumnSummary)>, columns: &[(ColumnPos, Versioned)]) {
    for (pos, column) in columns {
        if probes.len() >= WORLD_PROBE_COLUMNS {
            return;
        }
        if let Ok(decoded) = ColumnSummary::decode(&column.summary.bytes)
            && !decoded.is_empty()
        {
            probes.push((*pos, decoded));
        }
    }
}
