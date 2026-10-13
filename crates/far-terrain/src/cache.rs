//! One client's far-terrain cache: the active namespace in memory, persisted per region.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::PathBuf,
    time::Instant,
};

use crate::{
    Admission, ColumnPos, ColumnSummary, FarTerrainConfig, NamespaceKey, ServerKey, SummaryStore,
    WorldKey,
    config::{
        MAX_UNRESOLVED_COLUMNS, REGION_FLUSH_AFTER, REGION_FLUSH_COLUMNS, WORLD_PROBE_COLUMNS,
    },
    disk::{DiskBounds, DiskCache, DiskEvent},
    region::{REGION_SIDE, RegionPos, region_index},
};

#[derive(Debug)]
struct DirtyRegion {
    since: Instant,
    columns: BTreeSet<ColumnPos>,
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
    /// The namespace left by the last world change; late summaries for it go straight to disk.
    retired: Option<NamespaceKey>,
    unresolved: Vec<(ColumnPos, EncodedSummary)>,
    probes: Vec<(ColumnPos, ColumnSummary)>,
    resolving: bool,
    dirty: BTreeMap<RegionPos, DirtyRegion>,
    requested: HashSet<RegionPos>,
    focus: Option<(ColumnPos, i32)>,
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
            dirty: BTreeMap::new(),
            requested: HashSet::new(),
            focus: None,
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
        self.flush_all();
        self.retired = self.namespace.take();
        self.world = Some(world);
        self.store.clear();
        self.unresolved.clear();
        self.probes.clear();
        self.resolving = false;
        self.requested.clear();
    }

    /// Leaves the current world, as on disconnect, after writing what is waiting.
    pub fn leave_world(&mut self) {
        self.flush_all();
        self.retired = self.namespace.take();
        self.world = None;
        self.store.clear();
        self.unresolved.clear();
        self.probes.clear();
        self.resolving = false;
        self.requested.clear();
    }

    /// Records a column's encoded summary; equal content changes nothing.
    pub fn admit(&mut self, dimension: i32, pos: ColumnPos, summary: EncodedSummary, now: Instant) {
        if let Some(retired) = self
            .retired
            .filter(|retired| retired.dimension == dimension)
            && self.world.is_none_or(|world| world.dimension != dimension)
        {
            if let Some(disk) = &self.disk {
                disk.write(
                    retired,
                    RegionPos::of(pos),
                    vec![(region_index(pos), summary)],
                );
            }
            return;
        }
        if self.world.is_none_or(|world| world.dimension != dimension) {
            return;
        }
        if self.namespace.is_some() {
            self.admit_resolved(pos, summary, now);
            return;
        }
        if !self.resolving
            && self.probes.len() < WORLD_PROBE_COLUMNS
            && let Ok(decoded) = ColumnSummary::decode(&summary.bytes)
            && !decoded.is_empty()
        {
            self.probes.push((pos, decoded));
        }
        if self.unresolved.len() < MAX_UNRESOLVED_COLUMNS {
            self.unresolved.push((pos, summary));
        }
        if !self.resolving && self.probes.len() == WORLD_PROBE_COLUMNS {
            self.resolving = true;
            let world = self.world.expect("checked above");
            match &self.disk {
                Some(disk) => disk.resolve(world, std::mem::take(&mut self.probes)),
                None => self.resolved(world, 0, now),
            }
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

    /// Applies finished disk work and writes regions whose changes are due.
    pub fn poll(&mut self, now: Instant) {
        let events: Vec<DiskEvent> = self
            .disk
            .as_ref()
            .map(|disk| disk.events().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                DiskEvent::Resolved { world, epoch } => self.resolved(world, epoch, now),
                DiskEvent::Loaded {
                    namespace, columns, ..
                } => {
                    if self.namespace != Some(namespace) {
                        continue;
                    }
                    for (pos, summary) in columns {
                        if !self.store.contains(pos) {
                            self.store.admit_encoded(pos, summary);
                        }
                    }
                }
            }
        }
        let due: Vec<RegionPos> = self
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
        let regions: Vec<RegionPos> = self.dirty.keys().copied().collect();
        for region in regions {
            self.flush(region);
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

    /// Summaries of the active namespace.
    pub fn store(&mut self) -> &mut SummaryStore {
        &mut self.store
    }

    fn resolved(&mut self, world: WorldKey, epoch: u32, now: Instant) {
        if self.world != Some(world) || self.namespace.is_some() {
            return;
        }
        self.namespace = Some(world.namespace(epoch));
        for (pos, summary) in std::mem::take(&mut self.unresolved) {
            self.admit_resolved(pos, summary, now);
        }
        self.request_focus();
    }

    fn admit_resolved(&mut self, pos: ColumnPos, summary: EncodedSummary, now: Instant) {
        if self.store.admit_encoded(pos, summary) == Admission::Unchanged || self.disk.is_none() {
            return;
        }
        self.dirty
            .entry(RegionPos::of(pos))
            .or_insert_with(|| DirtyRegion {
                since: now,
                columns: BTreeSet::new(),
            })
            .columns
            .insert(pos);
    }

    fn flush(&mut self, region: RegionPos) {
        let Some(dirty) = self.dirty.remove(&region) else {
            return;
        };
        let (Some(disk), Some(namespace)) = (&self.disk, self.namespace) else {
            return;
        };
        let columns: Vec<_> = dirty
            .columns
            .into_iter()
            .filter_map(|pos| Some((region_index(pos), self.store.encoded(pos)?.clone())))
            .collect();
        if !columns.is_empty() {
            disk.write(namespace, region, columns);
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
        self.flush_all();
    }
}
