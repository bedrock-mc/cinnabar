//! The disk cache: one IO thread owns every file, enforces the caps and evicts by recency.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::SystemTime,
};

use crossbeam_channel::{Receiver, Sender, bounded};

mod admission;
use admission::{Admission, Envelope, MAX_PENDING_OPERATIONS};

#[cfg(test)]
mod tests;

use crate::{
    ColumnPos, ColumnSummary, NamespaceKey, ServerKey, Versioned, WorldKey,
    config::{WORLD_PROBE_EPOCHS, WORLD_PROBE_SIMILARITY},
    identity::surface_similarity,
    region::{RegionPos, decode_region, encode_region, region_index},
};

/// Per-server and global byte caps for the files under one root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskBounds {
    pub server_cap_bytes: u64,
    pub global_cap_bytes: u64,
}

/// Results the IO thread sends back.
#[derive(Debug)]
pub enum DiskEvent {
    /// A region's stored columns; empty when nothing is cached there.
    Loaded {
        namespace: NamespaceKey,
        region: RegionPos,
        columns: Vec<(ColumnPos, Versioned)>,
    },
    /// These column versions reached the region file, or were dropped if it could not be
    /// written; a read asked for afterwards sees them.
    Written {
        namespace: NamespaceKey,
        columns: Vec<(ColumnPos, u64)>,
    },
    /// The epoch whose cached terrain matches the probes, or a new one, for the caller's
    /// world-entry token.
    Resolved {
        world: WorldKey,
        entry: u64,
        epoch: u32,
    },
}

enum Command {
    Write {
        namespace: NamespaceKey,
        region: RegionPos,
        columns: Vec<(u16, Versioned)>,
    },
    Load {
        namespace: NamespaceKey,
        region: RegionPos,
    },
    Resolve {
        world: WorldKey,
        entry: u64,
        probes: Vec<(ColumnPos, ColumnSummary)>,
    },
    ResolveFresh {
        world: WorldKey,
        entry: u64,
        from: NamespaceKey,
        since: u64,
    },
    ResolveWrite {
        world: WorldKey,
        entry: u64,
        probes: Vec<(ColumnPos, ColumnSummary)>,
        columns: Vec<(ColumnPos, Versioned)>,
    },
    Sync(Sender<()>),
}

/// Handle to the IO thread; dropping it finishes admitted writes first.
/// Work submission returns `false` when retained commands or results fill admission;
/// the caller may retry without waiting on the frame thread.
pub struct DiskCache {
    tx: Option<Sender<Envelope<Command>>>,
    admission: Arc<Admission>,
    rx: Receiver<Envelope<DiskEvent>>,
    thread: Option<JoinHandle<()>>,
    observed_version: Arc<AtomicU64>,
}

impl DiskCache {
    /// Starts the IO thread for files under `root`, or returns `None` if it cannot start.
    #[must_use]
    pub fn spawn(root: PathBuf, bounds: DiskBounds) -> Option<Self> {
        fs::create_dir_all(&root).ok()?;
        let lease = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".writer.lock"))
            .ok()?;
        lease.try_lock().ok()?;
        let (tx, commands) = bounded::<Envelope<Command>>(MAX_PENDING_OPERATIONS);
        let (events, rx) = bounded(MAX_PENDING_OPERATIONS);
        let observed_version = Arc::new(AtomicU64::new(0));
        let worker_version = Arc::clone(&observed_version);
        let thread = std::thread::Builder::new()
            .name("far-terrain-io".into())
            .spawn(move || {
                // The descriptor owns the lease until all queued work has finished.
                let _lease = lease;
                let mut worker = Worker {
                    root,
                    bounds,
                    index: None,
                    claimed: HashSet::new(),
                    visits: HashMap::new(),
                    observed_version: worker_version,
                };
                for command in commands {
                    if let Some(event) = worker.run(command.value) {
                        let _ = events.send(Envelope {
                            value: event,
                            permit: command.permit,
                        });
                    }
                }
            })
            .ok()?;
        Some(Self {
            tx: Some(tx),
            admission: Arc::new(Admission::default()),
            rx,
            thread: Some(thread),
            observed_version,
        })
    }

    /// Stores the given slots in one region, keeping whichever version of each is higher.
    pub fn write(
        &self,
        namespace: NamespaceKey,
        region: RegionPos,
        columns: Vec<(u16, Versioned)>,
    ) -> bool {
        self.send(Command::Write {
            namespace,
            region,
            columns,
        })
    }

    /// Attempts a read without retaining more than the pending-work budget.
    pub fn load(&self, namespace: NamespaceKey, region: RegionPos) -> bool {
        self.send(Command::Load { namespace, region })
    }

    /// Picks the epoch of `world` whose cached columns match `probes`.
    pub fn resolve(
        &self,
        world: WorldKey,
        entry: u64,
        probes: Vec<(ColumnPos, ColumnSummary)>,
    ) -> bool {
        self.send(Command::Resolve {
            world,
            entry,
            probes,
        })
    }

    /// Gives visit `entry` to `world` a new epoch of its own, as when its cached columns turned
    /// out to be another map's, and moves the columns that visit already wrote to `from`
    /// (versions from `since` on) there.
    pub fn resolve_fresh(
        &self,
        world: WorldKey,
        entry: u64,
        from: NamespaceKey,
        since: u64,
    ) -> bool {
        self.send(Command::ResolveFresh {
            world,
            entry,
            from,
            since,
        })
    }

    /// Writes `columns` to the epoch of visit `entry` to `world`, resolving it from `probes`
    /// first if no earlier request of that visit did, for a world left before it resolved.
    pub fn resolve_and_write(
        &self,
        world: WorldKey,
        entry: u64,
        probes: Vec<(ColumnPos, ColumnSummary)>,
        columns: Vec<(ColumnPos, Versioned)>,
    ) -> bool {
        self.send(Command::ResolveWrite {
            world,
            entry,
            probes,
            columns,
        })
    }

    /// Events finished so far.
    pub fn events(&self) -> impl Iterator<Item = DiskEvent> + '_ {
        self.rx.try_iter().map(|envelope| envelope.value)
    }

    /// Highest version read by the worker, including events the owner has not consumed yet.
    pub(crate) fn observed_version(&self) -> u64 {
        self.observed_version.load(Ordering::Relaxed)
    }

    /// Blocks until every earlier command has finished.
    pub fn sync(&self) {
        let (done, wait) = crossbeam_channel::bounded(1);
        if let Some(tx) = &self.tx {
            let _ = tx.send(Envelope {
                value: Command::Sync(done),
                permit: None,
            });
            let _ = wait.recv();
        }
    }

    /// Optional cache work is refused immediately when commands or results fill admission.
    fn send(&self, command: Command) -> bool {
        let Some(permit) = self.admission.acquire(command.retained_bytes()) else {
            return false;
        };
        self.tx.as_ref().is_some_and(|tx| {
            tx.try_send(Envelope {
                value: command,
                permit: Some(permit),
            })
            .is_ok()
        })
    }
}

impl Drop for DiskCache {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct FileEntry {
    server: ServerKey,
    bytes: u64,
    stamp: u64,
}

/// Every region file under the root with its size and recency order.
#[derive(Default)]
struct FileIndex {
    files: HashMap<PathBuf, FileEntry>,
    next_stamp: u64,
}

struct Worker {
    root: PathBuf,
    bounds: DiskBounds,
    /// Built on first use, so a session that only reads never walks the whole cache.
    index: Option<FileIndex>,
    /// Namespaces handed out by this run, so two worlds resolved before either writes
    /// never share one.
    claimed: HashSet<NamespaceKey>,
    /// The epoch each world visit resolved to, so every later write of that visit lands in it.
    visits: HashMap<(WorldKey, u64), u32>,
    observed_version: Arc<AtomicU64>,
}

impl Worker {
    fn run(&mut self, command: Command) -> Option<DiskEvent> {
        match command {
            Command::Write {
                namespace,
                region,
                columns,
            } => {
                let written = columns
                    .iter()
                    .map(|(slot, column)| (region.column(*slot), column.version))
                    .collect();
                self.write(namespace, region, columns);
                Some(DiskEvent::Written {
                    namespace,
                    columns: written,
                })
            }
            Command::Load { namespace, region } => {
                let columns = self.load(namespace, region);
                let highest = columns
                    .iter()
                    .map(|(_, column)| column.version)
                    .max()
                    .unwrap_or(0);
                self.observed_version.fetch_max(highest, Ordering::Relaxed);
                Some(DiskEvent::Loaded {
                    namespace,
                    region,
                    columns,
                })
            }
            Command::Resolve {
                world,
                entry,
                probes,
            } => {
                let epoch = self.resolve_entry(world, entry, &probes);
                Some(DiskEvent::Resolved {
                    world,
                    entry,
                    epoch,
                })
            }
            Command::ResolveFresh {
                world,
                entry,
                from,
                since,
            } => {
                let epoch = self.fresh_epoch(world);
                self.claim(world.namespace(epoch));
                self.visits.insert((world, entry), epoch);
                self.move_newer(from, world.namespace(epoch), since);
                Some(DiskEvent::Resolved {
                    world,
                    entry,
                    epoch,
                })
            }
            Command::ResolveWrite {
                world,
                entry,
                probes,
                columns,
            } => {
                let namespace = world.namespace(self.resolve_entry(world, entry, &probes));
                let mut regions = BTreeMap::<RegionPos, Vec<(u16, Versioned)>>::new();
                for (pos, column) in columns {
                    regions
                        .entry(RegionPos::of(pos))
                        .or_default()
                        .push((region_index(pos), column));
                }
                for (region, columns) in regions {
                    self.write(namespace, region, columns);
                }
                None
            }
            Command::Sync(done) => {
                let _ = done.send(());
                None
            }
        }
    }

    fn region_path(&self, namespace: NamespaceKey, region: RegionPos) -> PathBuf {
        self.root
            .join(namespace.server.to_string())
            .join(namespace.directory())
            .join(region.file_name())
    }

    fn write(
        &mut self,
        namespace: NamespaceKey,
        region: RegionPos,
        columns: Vec<(u16, Versioned)>,
    ) {
        let path = self.region_path(namespace, region);
        let mut merged = read_region(&path).unwrap_or_default();
        merged.extend(columns);
        let bytes = encode_region(&merged);
        if let Err(error) = write_atomic(&path, &bytes) {
            tracing::warn!(server = %namespace.server, %error, "far-terrain region write failed");
            return;
        }
        self.index()
            .record(path, namespace.server, bytes.len() as u64);
        self.enforce_caps();
    }

    fn load(&mut self, namespace: NamespaceKey, region: RegionPos) -> Vec<(ColumnPos, Versioned)> {
        let path = self.region_path(namespace, region);
        if !path.exists() {
            return Vec::new();
        }
        match read_region(&path) {
            Some(columns) => {
                if let Ok(file) = fs::File::options().append(true).open(&path) {
                    let _ = file.set_modified(SystemTime::now());
                }
                if let Some(index) = &mut self.index {
                    index.touch(&path);
                }
                columns
                    .into_iter()
                    .map(|(slot, summary)| (region.column(slot), summary))
                    .collect()
            }
            None => {
                if remove_cached_file(&path)
                    && let Some(index) = &mut self.index
                {
                    index.files.remove(&path);
                }
                Vec::new()
            }
        }
    }

    /// The epoch of visit `entry` to `world`, resolved by its first request and reused after.
    fn resolve_entry(
        &mut self,
        world: WorldKey,
        entry: u64,
        probes: &[(ColumnPos, ColumnSummary)],
    ) -> u32 {
        if let Some(&epoch) = self.visits.get(&(world, entry)) {
            return epoch;
        }
        let epoch = self.resolve(world, probes);
        self.visits.insert((world, entry), epoch);
        epoch
    }

    fn resolve(&mut self, world: WorldKey, probes: &[(ColumnPos, ColumnSummary)]) -> u32 {
        let epoch = self.pick_epoch(world, probes);
        self.claim(world.namespace(epoch));
        epoch
    }

    /// Reuses an epoch only when every live probe has a matching cached column.
    /// Missing evidence starts a fresh epoch rather than showing another map's terrain.
    fn pick_epoch(&self, world: WorldKey, probes: &[(ColumnPos, ColumnSummary)]) -> u32 {
        let mut epochs: Vec<u32> = fs::read_dir(self.root.join(world.server.to_string()))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| NamespaceKey::parse(world.server, entry.file_name().to_str()?))
            .chain(self.claimed.iter().copied())
            .filter(|namespace| namespace.world() == world)
            .map(|namespace| namespace.epoch)
            .collect();
        epochs.sort_unstable_by(|a, b| b.cmp(a));
        epochs.dedup();
        let Some(&newest) = epochs.first() else {
            return 0;
        };
        for &epoch in epochs.iter().take(WORLD_PROBE_EPOCHS) {
            let (present, similar) = self.compare(world.namespace(epoch), probes);
            if !probes.is_empty() && present == probes.len() && similar == present {
                return epoch;
            }
        }
        newest + 1
    }

    /// Moves every column of `from` at version `since` or newer into `to`, so a visit that
    /// turned out to be another map keeps what it wrote and leaves `from` as it was.
    fn move_newer(&mut self, from: NamespaceKey, to: NamespaceKey, since: u64) {
        let directory = self
            .root
            .join(from.server.to_string())
            .join(from.directory());
        for entry in fs::read_dir(directory).into_iter().flatten().flatten() {
            let Some(region) = entry.file_name().to_str().and_then(RegionPos::parse) else {
                continue;
            };
            let path = entry.path();
            let Some(columns) = read_region(&path) else {
                continue;
            };
            let (moved, kept): (Vec<_>, Vec<_>) = columns
                .into_iter()
                .partition(|(_, column)| column.version >= since);
            if moved.is_empty() {
                continue;
            }
            if kept.is_empty() {
                let _ = fs::remove_file(&path);
                if let Some(index) = &mut self.index {
                    index.files.remove(&path);
                }
            } else {
                let bytes = encode_region(&kept);
                if write_atomic(&path, &bytes).is_ok() {
                    self.index().record(path, from.server, bytes.len() as u64);
                }
            }
            self.write(to, region, moved);
        }
    }

    /// An epoch past every one of `world` on disk or handed out this run.
    fn fresh_epoch(&self, world: WorldKey) -> u32 {
        fs::read_dir(self.root.join(world.server.to_string()))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| NamespaceKey::parse(world.server, entry.file_name().to_str()?))
            .chain(self.claimed.iter().copied())
            .filter(|namespace| namespace.world() == world)
            .map(|namespace| namespace.epoch + 1)
            .max()
            .unwrap_or(0)
    }

    /// Reserves `namespace` before it is acknowledged, on disk as well as for this run.
    fn claim(&mut self, namespace: NamespaceKey) {
        let directory = self
            .root
            .join(namespace.server.to_string())
            .join(namespace.directory());
        if let Err(error) = fs::create_dir_all(&directory) {
            tracing::warn!(server = %namespace.server, %error, "far-terrain epoch reservation failed");
        }
        self.claimed.insert(namespace);
    }

    /// Counts probes the namespace holds, and how many of those match closely.
    fn compare(
        &self,
        namespace: NamespaceKey,
        probes: &[(ColumnPos, ColumnSummary)],
    ) -> (usize, usize) {
        let mut regions = BTreeMap::<RegionPos, Vec<(u16, Versioned)>>::new();
        let (mut present, mut similar) = (0, 0);
        for (pos, probe) in probes {
            let region = RegionPos::of(*pos);
            let columns = regions.entry(region).or_insert_with(|| {
                read_region(&self.region_path(namespace, region)).unwrap_or_default()
            });
            let slot = region_index(*pos);
            let Some(cached) = columns
                .iter()
                .rev()
                .find(|(index, _)| *index == slot)
                .and_then(|(_, column)| ColumnSummary::decode(&column.summary.bytes).ok())
            else {
                continue;
            };
            present += 1;
            if surface_similarity(&cached, probe) >= WORLD_PROBE_SIMILARITY {
                similar += 1;
            }
        }
        (present, similar)
    }

    fn index(&mut self) -> &mut FileIndex {
        let root = &self.root;
        self.index.get_or_insert_with(|| FileIndex::scan(root))
    }

    /// Removes the least recently used regions until every server and the total fit.
    fn enforce_caps(&mut self) {
        self.enforce_caps_with(remove_cached_file);
    }

    /// Keeps failed deletions in the index so later passes can retry their real bytes.
    fn enforce_caps_with(&mut self, mut remove: impl FnMut(&Path) -> bool) {
        let bounds = self.bounds;
        let index = self.index();
        let mut by_age: Vec<(PathBuf, FileEntry)> = index
            .files
            .iter()
            .map(|(path, entry)| (path.clone(), *entry))
            .collect();
        by_age.sort_unstable_by_key(|(_, entry)| entry.stamp);
        let mut per_server = HashMap::<ServerKey, u64>::new();
        let mut total = 0;
        for (_, entry) in &by_age {
            *per_server.entry(entry.server).or_default() += entry.bytes;
            total += entry.bytes;
        }
        for (path, entry) in by_age {
            let server_total = per_server[&entry.server];
            if server_total <= bounds.server_cap_bytes && total <= bounds.global_cap_bytes {
                continue;
            }
            if !remove(&path) {
                continue;
            }
            index.files.remove(&path);
            *per_server.get_mut(&entry.server).expect("counted above") -= entry.bytes;
            total -= entry.bytes;
            if let Some(namespace) = path.parent() {
                let _ = fs::remove_dir(namespace);
                if let Some(server) = namespace.parent() {
                    let _ = fs::remove_dir(server);
                }
            }
        }
        if total > bounds.global_cap_bytes
            || per_server
                .values()
                .any(|bytes| *bytes > bounds.server_cap_bytes)
        {
            tracing::warn!(total, "far-terrain cache could not meet its disk quota");
        }
    }
}

impl FileIndex {
    /// Lists `root/<server>/<namespace>/<region>` files, oldest modification first.
    fn scan(root: &Path) -> Self {
        let mut found = Vec::new();
        for server_dir in fs::read_dir(root).into_iter().flatten().flatten() {
            let Some(server) = server_dir
                .file_name()
                .to_str()
                .and_then(ServerKey::from_hex)
            else {
                continue;
            };
            for namespace in fs::read_dir(server_dir.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                for region in fs::read_dir(namespace.path())
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    let path = region.path();
                    let is_region = region
                        .file_name()
                        .to_str()
                        .is_some_and(|name| RegionPos::parse(name).is_some());
                    let Ok(metadata) = region.metadata() else {
                        continue;
                    };
                    if !is_region || !metadata.is_file() {
                        continue;
                    }
                    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    found.push((modified, path, server, metadata.len()));
                }
            }
        }
        found.sort_by_key(|(modified, ..)| *modified);
        let mut index = Self::default();
        for (_, path, server, bytes) in found {
            index.record(path, server, bytes);
        }
        index
    }

    fn record(&mut self, path: PathBuf, server: ServerKey, bytes: u64) {
        self.next_stamp += 1;
        self.files.insert(
            path,
            FileEntry {
                server,
                bytes,
                stamp: self.next_stamp,
            },
        );
    }

    fn touch(&mut self, path: &Path) {
        self.next_stamp += 1;
        if let Some(entry) = self.files.get_mut(path) {
            entry.stamp = self.next_stamp;
        }
    }
}

fn read_region(path: &Path) -> Option<Vec<(u16, Versioned)>> {
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > crate::region::MAX_REGION_BYTES as u64 {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(crate::region::MAX_REGION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    decode_region(&bytes).ok()
}

/// Treats an already absent file as removed, retaining every other failure for retry.
fn remove_cached_file(path: &Path) -> bool {
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "far-terrain cache eviction failed");
            false
        }
    }
}

/// Writes through a sibling temporary file so readers never see a partial region.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("lod.tmp");
    let result = fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
