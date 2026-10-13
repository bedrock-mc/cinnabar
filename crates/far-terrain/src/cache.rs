//! One client's far-terrain cache: the active namespace in memory, persisted per region.
//!
//! Every summary carries a version stamped when it was taken, from a counter that only rises
//! across runs. Memory, waiting writes, writes in flight, region files and disk reads all keep
//! the higher version of a column, so an older summary never replaces a newer one wherever and
//! however late it arrives.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    time::Instant,
};

use crate::{
    ColumnPos, ColumnSummary, EncodedSummary, FarTerrainConfig, NamespaceKey, ServerKey,
    SummaryStore, Versioned, WorldKey,
    config::{
        MAX_UNRESOLVED_COLUMNS, REGION_FLUSH_AFTER, REGION_FLUSH_COLUMNS, WORLD_PROBE_COLUMNS,
        WORLD_PROBE_TIMEOUT,
    },
    disk::{DiskBounds, DiskCache, DiskEvent},
    region::{REGION_SIDE, RegionPos, region_index},
};

#[derive(Debug)]
struct DirtyRegion {
    since: Instant,
    /// Payloads are kept here, not looked up at flush, so memory eviction cannot lose them.
    columns: BTreeMap<ColumnPos, Versioned>,
}

/// A session whose final summaries are still being extracted, from
/// [`FarTerrainCache::retire_session`]; hand it back to [`FarTerrainCache::persist`]. Its
/// summaries carry the version stamped at retirement, so anything taken later wins.
#[derive(Debug)]
#[must_use]
pub struct RetiringSession {
    world: WorldKey,
    namespace: Option<NamespaceKey>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    version: u64,
}

impl RetiringSession {
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
    /// The namespace left by the last world change; late summaries for it go straight to disk.
    retired: Option<NamespaceKey>,
    /// Columns shown before the namespace resolved, written once it does.
    unresolved: Vec<(ColumnPos, Versioned)>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    resolving: bool,
    /// When this visit's first column arrived, for [`WORLD_PROBE_TIMEOUT`].
    pending_since: Option<Instant>,
    /// Changes waiting to be written, per namespace and region.
    dirty: BTreeMap<(NamespaceKey, RegionPos), DirtyRegion>,
    /// Changes handed to the IO thread and not yet reported written.
    writing: HashMap<(NamespaceKey, ColumnPos), Versioned>,
    requested: HashSet<RegionPos>,
    focus: Option<(ColumnPos, i32)>,
    /// Bumped on every world change, so a resolution for an earlier visit is ignored.
    entry: u64,
    next_version: u64,
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
            writing: HashMap::new(),
            requested: HashSet::new(),
            focus: None,
            entry: 0,
            next_version: version_base,
        }
    }

    /// Switches to `dimension` of the level `level` on the server at `destination`.
    /// Anything else, including a transfer, flushes and leaves the previous namespace.
    pub fn enter_world(&mut self, destination: &str, level: u64, dimension: i32) {
        let world = WorldKey {
            server: ServerKey::from_destination(destination),
            dimension,
            level,
        };
        if self.world == Some(world) {
            return;
        }
        self.end_entry();
        if let Some((root, bounds)) = self.disk_root.take() {
            self.disk = DiskCache::spawn(root, bounds);
        }
        self.world = Some(world);
    }

    /// Leaves the current world, as on disconnect, after writing what is waiting.
    pub fn leave_world(&mut self) {
        self.end_entry();
        self.world = None;
    }

    /// Writes everything the visit holds, resolving its namespace on the IO thread with the
    /// probes it has when it never resolved.
    fn end_entry(&mut self) {
        self.flush_all();
        if self.namespace.is_none()
            && let (Some(world), Some(disk)) = (self.world, &self.disk)
            && !self.unresolved.is_empty()
        {
            disk.resolve_and_write(
                world,
                std::mem::take(&mut self.probes),
                std::mem::take(&mut self.unresolved),
            );
        }
        self.retired = self.namespace.take();
        self.entry += 1;
        self.store.clear();
        self.unresolved.clear();
        self.probes.clear();
        self.resolving = false;
        self.pending_since = None;
        self.requested.clear();
    }

    /// Records a column's encoded summary as the newest of it. Columns show at once and are
    /// written once the world's namespace is resolved.
    pub fn admit(&mut self, dimension: i32, pos: ColumnPos, summary: EncodedSummary, now: Instant) {
        let column = Versioned {
            version: self.stamp(),
            summary,
        };
        if let Some(retired) = self
            .retired
            .filter(|retired| retired.dimension == dimension)
            && self.world.is_none_or(|world| world.dimension != dimension)
        {
            self.queue_write(retired, pos, column, now);
            return;
        }
        if self.world.is_none_or(|world| world.dimension != dimension) {
            return;
        }
        if let Some(namespace) = self.namespace {
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
            self.start_resolving(now);
        }
    }

    /// Loads cached regions within `radius` chunks of `center` once each.
    pub fn set_focus(&mut self, center: ColumnPos, radius: i32) {
        if self.focus == Some((center, radius)) {
            return;
        }
        self.focus = Some((center, radius));
        self.request_focus();
    }

    /// Applies finished disk work, resolves a sparse world after [`WORLD_PROBE_TIMEOUT`] and
    /// writes regions whose changes are due.
    pub fn poll(&mut self, now: Instant) {
        let events: Vec<DiskEvent> = self
            .disk
            .as_ref()
            .map(|disk| disk.events().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                DiskEvent::Resolved {
                    world,
                    entry,
                    epoch,
                } => {
                    if entry == self.entry {
                        self.resolved(world, epoch, now);
                    }
                }
                DiskEvent::Loaded {
                    namespace,
                    region,
                    columns,
                } => self.loaded(namespace, region, columns),
                DiskEvent::Written { namespace, columns } => {
                    for (pos, version) in columns {
                        if self
                            .writing
                            .get(&(namespace, pos))
                            .is_some_and(|writing| writing.version <= version)
                        {
                            self.writing.remove(&(namespace, pos));
                        }
                    }
                }
            }
        }
        if self.namespace.is_none()
            && !self.resolving
            && self
                .pending_since
                .is_some_and(|since| now.saturating_duration_since(since) >= WORLD_PROBE_TIMEOUT)
        {
            self.start_resolving(now);
        }
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
        let RetiringSession {
            world,
            namespace,
            mut probes,
            version,
        } = session;
        let mut unresolved = Vec::new();
        for (pos, summary) in columns {
            let column = Versioned { version, summary };
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
            for (pos, column) in &unresolved {
                if probes.len() >= WORLD_PROBE_COLUMNS {
                    break;
                }
                if let Ok(decoded) = ColumnSummary::decode(&column.summary.bytes)
                    && !decoded.is_empty()
                {
                    probes.push((*pos, decoded));
                }
            }
            disk.resolve_and_write(world, probes, unresolved);
        }
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

    fn stamp(&mut self) -> u64 {
        self.next_version += 1;
        self.next_version
    }

    fn start_resolving(&mut self, now: Instant) {
        let Some(world) = self.world else {
            return;
        };
        self.resolving = true;
        match &self.disk {
            Some(disk) => disk.resolve(world, self.entry, self.probes.clone()),
            None => self.resolved(world, 0, now),
        }
    }

    fn resolved(&mut self, world: WorldKey, epoch: u32, now: Instant) {
        if self.world != Some(world) || self.namespace.is_some() {
            return;
        }
        let namespace = world.namespace(epoch);
        self.namespace = Some(namespace);
        for (pos, column) in std::mem::take(&mut self.unresolved) {
            self.queue_write(namespace, pos, column, now);
        }
        self.request_focus();
    }

    /// Installs a region read; changes waiting or in flight to that region take part, and the
    /// highest version of each column wins.
    fn loaded(
        &mut self,
        namespace: NamespaceKey,
        region: RegionPos,
        columns: Vec<(ColumnPos, Versioned)>,
    ) {
        for (_, column) in &columns {
            self.next_version = self.next_version.max(column.version);
        }
        if self.namespace != Some(namespace) {
            return;
        }
        let waiting: Vec<(ColumnPos, Versioned)> = self
            .dirty
            .get(&(namespace, region))
            .into_iter()
            .flat_map(|dirty| {
                dirty
                    .columns
                    .iter()
                    .map(|(pos, column)| (*pos, column.clone()))
            })
            .collect();
        let in_flight: Vec<(ColumnPos, Versioned)> = self
            .writing
            .iter()
            .filter(|((written, pos), _)| *written == namespace && RegionPos::of(*pos) == region)
            .map(|((_, pos), column)| (*pos, column.clone()))
            .collect();
        for (pos, column) in columns.into_iter().chain(waiting).chain(in_flight) {
            self.store.admit_versioned(pos, column);
        }
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
                .entry((key.0, pos))
                .or_insert_with(|| column.clone());
            if in_flight.version < column.version {
                *in_flight = column.clone();
            }
            columns.push((region_index(pos), column));
        }
        disk.write(key.0, key.1, columns);
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
