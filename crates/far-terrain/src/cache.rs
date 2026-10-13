//! One client's far-terrain cache: the active namespace in memory, persisted per region.
//!
//! Each world visit is an entry: pending until its namespace resolves, then active. Columns
//! show from memory at once; writes wait for the namespace. Disk reads never replace a column
//! touched since the read was asked for, and a retiring session's late summaries never
//! replace what a later visit wrote.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    time::Instant,
};

use crate::{
    Admission, ColumnPos, ColumnSummary, EncodedSummary, FarTerrainConfig, NamespaceKey, ServerKey,
    SummaryStore, WorldKey,
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
    columns: BTreeMap<ColumnPos, EncodedSummary>,
}

/// A session whose final summaries are still being extracted, from
/// [`FarTerrainCache::retire_session`]; hand it back to [`FarTerrainCache::persist`].
#[derive(Debug)]
#[must_use]
pub struct RetiringSession {
    namespace: NamespaceKey,
    entry: u64,
}

impl RetiringSession {
    #[must_use]
    pub const fn dimension(&self) -> i32 {
        self.namespace.dimension
    }
}

/// Summaries of the world the client is in, with reads and batched writes on an IO thread.
///
/// A world's namespace is resolved from its first non-empty columns, so a server that reuses
/// one level identity for different maps gets a fresh namespace instead of mixed terrain.
pub struct FarTerrainCache {
    disk: Option<DiskCache>,
    store: SummaryStore,
    world: Option<WorldKey>,
    namespace: Option<NamespaceKey>,
    /// The namespace left by the last world change and its entry; late summaries for it go
    /// straight to disk.
    retired: Option<(NamespaceKey, u64)>,
    unresolved: Vec<(ColumnPos, EncodedSummary)>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    resolving: bool,
    /// When this entry's first column arrived, for [`WORLD_PROBE_TIMEOUT`].
    pending_since: Option<Instant>,
    /// Changes waiting to be written, per namespace and region.
    dirty: BTreeMap<(NamespaceKey, RegionPos), DirtyRegion>,
    requested: HashSet<RegionPos>,
    /// Region reads in flight, with the touch sequence when each was asked for.
    loads: HashMap<(NamespaceKey, RegionPos), u64>,
    /// Last admission or write hand-off of each active column, kept while a read asked for
    /// earlier is in flight.
    touched: HashMap<ColumnPos, u64>,
    sequence: u64,
    /// Entry of the newest write queued per column while a retiring session may persist.
    written: HashMap<(NamespaceKey, ColumnPos), u64>,
    retiring: usize,
    focus: Option<(ColumnPos, i32)>,
    /// Bumped on every world change, so work for an earlier visit is told apart.
    entry: u64,
}

impl FarTerrainCache {
    /// A cache persisted under `disk_root`, or memory-only without one.
    #[must_use]
    pub fn new(config: FarTerrainConfig, disk_root: Option<PathBuf>) -> Self {
        let bounds = DiskBounds {
            server_cap_bytes: config.server_disk_cap_bytes,
            global_cap_bytes: config.global_disk_cap_bytes,
        };
        Self {
            disk: disk_root.and_then(|root| DiskCache::spawn(root, bounds)),
            store: SummaryStore::new(config.memory_budget_bytes),
            world: None,
            namespace: None,
            retired: None,
            unresolved: Vec::new(),
            probes: Vec::new(),
            resolving: false,
            pending_since: None,
            dirty: BTreeMap::new(),
            requested: HashSet::new(),
            loads: HashMap::new(),
            touched: HashMap::new(),
            sequence: 0,
            written: HashMap::new(),
            retiring: 0,
            focus: None,
            entry: 0,
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
        self.world = Some(world);
    }

    /// Leaves the current world, as on disconnect, after writing what is waiting.
    pub fn leave_world(&mut self) {
        self.end_entry();
        self.world = None;
    }

    fn end_entry(&mut self) {
        self.flush_all();
        self.retired = self
            .namespace
            .take()
            .map(|namespace| (namespace, self.entry));
        self.entry += 1;
        self.store.clear();
        self.unresolved.clear();
        self.probes.clear();
        self.resolving = false;
        self.pending_since = None;
        self.requested.clear();
        self.loads.clear();
        self.touched.clear();
    }

    /// Records a column's encoded summary; equal content changes nothing. Columns show at
    /// once and are written once the world's namespace is resolved.
    pub fn admit(&mut self, dimension: i32, pos: ColumnPos, summary: EncodedSummary, now: Instant) {
        if let Some((retired, entry)) = self
            .retired
            .filter(|(retired, _)| retired.dimension == dimension)
            && self.world.is_none_or(|world| world.dimension != dimension)
        {
            self.queue_write(retired, entry, pos, summary, now);
            return;
        }
        if self.world.is_none_or(|world| world.dimension != dimension) {
            return;
        }
        self.touch(pos);
        if let Some(namespace) = self.namespace {
            let pending = self.disk.is_some().then(|| summary.clone());
            if self.store.admit_encoded(pos, summary) != Admission::Unchanged
                && let Some(pending) = pending
            {
                self.queue_write(namespace, self.entry, pos, pending, now);
            }
            return;
        }
        self.pending_since.get_or_insert(now);
        if !self.resolving
            && self.probes.len() < WORLD_PROBE_COLUMNS
            && let Ok(decoded) = ColumnSummary::decode(&summary.bytes)
            && !decoded.is_empty()
        {
            self.probes.push((pos, decoded));
        }
        let pending = (self.disk.is_some() && self.unresolved.len() < MAX_UNRESOLVED_COLUMNS)
            .then(|| summary.clone());
        if self.store.admit_encoded(pos, summary) != Admission::Unchanged
            && let Some(pending) = pending
        {
            self.unresolved.push((pos, pending));
        }
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
        if self.retiring == 0 {
            self.written.clear();
        }
        match self.loads.values().min().copied() {
            None => self.touched.clear(),
            Some(oldest) => self.touched.retain(|_, touched| *touched > oldest),
        }
    }

    /// Writes every waiting change now.
    pub fn flush_all(&mut self) {
        let regions: Vec<(NamespaceKey, RegionPos)> = self.dirty.keys().copied().collect();
        for region in regions {
            self.flush(region);
        }
    }

    /// Starts a session's retirement while its final summaries are extracted; `None` before
    /// its namespace is resolved.
    pub fn retire_session(&mut self) -> Option<RetiringSession> {
        let namespace = self.namespace?;
        self.retiring += 1;
        Some(RetiringSession {
            namespace,
            entry: self.entry,
        })
    }

    /// Writes a retired session's final summaries to its namespace, skipping columns a later
    /// visit has written since, and shows them when that namespace is active again.
    pub fn persist(
        &mut self,
        session: RetiringSession,
        columns: impl IntoIterator<Item = (ColumnPos, EncodedSummary)>,
        now: Instant,
    ) {
        let RetiringSession { namespace, entry } = session;
        self.retiring = self.retiring.saturating_sub(1);
        let active = self.namespace == Some(namespace);
        for (pos, summary) in columns {
            if self
                .written
                .get(&(namespace, pos))
                .is_some_and(|&written| written > entry)
            {
                continue;
            }
            if active {
                self.touch(pos);
                self.store.admit_encoded(pos, summary.clone());
            }
            self.queue_write(namespace, entry, pos, summary, now);
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

    fn touch(&mut self, pos: ColumnPos) {
        self.sequence += 1;
        self.touched.insert(pos, self.sequence);
    }

    fn start_resolving(&mut self, now: Instant) {
        let Some(world) = self.world else {
            return;
        };
        self.resolving = true;
        match &self.disk {
            Some(disk) => disk.resolve(world, self.entry, std::mem::take(&mut self.probes)),
            None => self.resolved(world, 0, now),
        }
    }

    fn resolved(&mut self, world: WorldKey, epoch: u32, now: Instant) {
        if self.world != Some(world) || self.namespace.is_some() {
            return;
        }
        let namespace = world.namespace(epoch);
        self.namespace = Some(namespace);
        for (pos, summary) in std::mem::take(&mut self.unresolved) {
            self.queue_write(namespace, self.entry, pos, summary, now);
        }
        self.request_focus();
    }

    /// Installs a region read, except columns touched since it was asked for or already
    /// shown; changes still waiting to be written win over what the file held.
    fn loaded(
        &mut self,
        namespace: NamespaceKey,
        region: RegionPos,
        columns: Vec<(ColumnPos, EncodedSummary)>,
    ) {
        let asked = self.loads.remove(&(namespace, region));
        if self.namespace != Some(namespace) {
            return;
        }
        let asked = asked.unwrap_or(self.sequence);
        let waiting = self
            .dirty
            .get(&(namespace, region))
            .map(|dirty| &dirty.columns);
        let touched = &self.touched;
        let newer = |pos: ColumnPos| touched.get(&pos).is_some_and(|&touched| touched > asked);
        let mut stale = false;
        let from_disk = columns.into_iter().map(|(pos, summary)| {
            match waiting.and_then(|waiting| waiting.get(&pos)) {
                Some(waiting) => (pos, waiting.clone(), true),
                None => (pos, summary, false),
            }
        });
        let only_waiting = waiting
            .into_iter()
            .flatten()
            .map(|(pos, summary)| (*pos, summary.clone(), true));
        for (pos, summary, queued) in from_disk.chain(only_waiting) {
            if self.store.contains(pos) {
                continue;
            }
            if !queued && newer(pos) {
                // Handed to the disk after this read was served: read the region again.
                stale = true;
                continue;
            }
            self.store.admit_encoded(pos, summary);
        }
        if stale && let Some(disk) = &self.disk {
            self.loads.insert((namespace, region), self.sequence);
            disk.load(namespace, region);
        }
    }

    fn queue_write(
        &mut self,
        namespace: NamespaceKey,
        entry: u64,
        pos: ColumnPos,
        summary: EncodedSummary,
        now: Instant,
    ) {
        if self.disk.is_none() {
            return;
        }
        if self.retiring > 0 {
            let written = self.written.entry((namespace, pos)).or_insert(entry);
            *written = (*written).max(entry);
        }
        self.dirty
            .entry((namespace, RegionPos::of(pos)))
            .or_insert_with(|| DirtyRegion {
                since: now,
                columns: BTreeMap::new(),
            })
            .columns
            .insert(pos, summary);
    }

    fn flush(&mut self, key: (NamespaceKey, RegionPos)) {
        let (Some(dirty), Some(disk)) = (self.dirty.remove(&key), &self.disk) else {
            return;
        };
        // A read already in flight may be served before this write lands.
        if self.namespace == Some(key.0) && !self.loads.is_empty() {
            self.sequence += 1;
            for pos in dirty.columns.keys() {
                self.touched.insert(*pos, self.sequence);
            }
        }
        let columns: Vec<_> = dirty
            .columns
            .into_iter()
            .map(|(pos, summary)| (region_index(pos), summary))
            .collect();
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
                    self.loads.insert((namespace, region), self.sequence);
                    disk.load(namespace, region);
                }
            }
        }
    }
}

impl Drop for FarTerrainCache {
    fn drop(&mut self) {
        self.flush_all();
    }
}
