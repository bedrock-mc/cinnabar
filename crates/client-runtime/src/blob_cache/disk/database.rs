use super::*;
use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::path::Path;

pub(super) const BLOBS: TableDefinition<u64, &[u8]> = TableDefinition::new("blobs-v1");
// Length and a persisted logical access timestamp; survives wall-clock changes and restarts.
pub(super) const ACCESS: TableDefinition<u64, (u64, u64)> = TableDefinition::new("access-v1");
const RECORD_BYTES: u64 = 32;
const PAGE_CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(super) struct Entry {
    pub(super) bytes: u64,
    pub(super) stamp: u64,
}

pub(super) struct Store {
    pub(super) db: Database,
    pub(super) index: HashMap<u64, Entry>,
    cap: u64,
    pub(super) bytes: u64,
}

impl Store {
    /// Opens and scans only the metadata table; no blob body is trusted by its index entry.
    pub(super) fn open(path: &Path, cap: u64) -> Result<Self> {
        std::fs::create_dir_all(path.parent().context("blob cache path has no parent")?)?;
        let backend = file::CappedFile::open(path, cap)?;
        let db = Database::builder()
            .set_cache_size(PAGE_CACHE_BYTES)
            .create_with_backend(backend)?;
        let transaction = db.begin_write()?;
        transaction.open_table(BLOBS)?;
        transaction.open_table(ACCESS)?;
        transaction.commit()?;
        let mut store = Self {
            db,
            index: HashMap::new(),
            cap,
            bytes: 0,
        };
        let read = store.db.begin_read()?;
        let access = read.open_table(ACCESS)?;
        for record in access.iter()? {
            let (hash, value) = record?;
            let (bytes, stamp) = value.value();
            anyhow::ensure!(
                store.index.len() < MAX_DISK_ENTRIES,
                "blob cache index exceeds its entry limit"
            );
            anyhow::ensure!(
                bytes <= protocol::MAX_CLIENT_BLOB_RECONSTRUCTED_BYTES as u64,
                "blob cache entry exceeds its body limit"
            );
            store.bytes = store.bytes.saturating_add(bytes + RECORD_BYTES);
            store.index.insert(hash.value(), Entry { bytes, stamp });
        }
        drop(access);
        drop(read);
        // Reapply current limits when opening a store written under a different configuration.
        store.flush(HashMap::new(), HashMap::new())?;
        Ok(store)
    }

    /// Reads one bounded body; hash validation and memory admission happen outside the database.
    pub(super) fn read(&self, hash: u64) -> Result<Option<Arc<[u8]>>> {
        let Some(entry) = self.index.get(&hash) else {
            return Ok(None);
        };
        let read = self.db.begin_read()?;
        let table = read.open_table(BLOBS)?;
        let Some(value) = table.get(hash)? else {
            return Ok(None);
        };
        if value.value().len() as u64 != entry.bytes {
            return Ok(None);
        }
        Ok(Some(Arc::from(value.value())))
    }

    /// Deletes a corrupt or incomplete entry atomically; a later request follows the miss path.
    pub(super) fn remove(&mut self, hash: u64) -> Result<()> {
        let write = self.db.begin_write()?;
        write.open_table(BLOBS)?.remove(hash)?;
        write.open_table(ACCESS)?.remove(hash)?;
        write.commit()?;
        if let Some(entry) = self.index.remove(&hash) {
            self.bytes = self.bytes.saturating_sub(entry.bytes + RECORD_BYTES);
        }
        Ok(())
    }

    /// Commits a coalesced insert/access batch and evicts oldest access stamps in that transaction.
    pub(super) fn flush(
        &mut self,
        writes: HashMap<u64, (Arc<[u8]>, u64)>,
        touches: HashMap<u64, u64>,
    ) -> Result<Vec<u64>> {
        let mut evicted = Vec::new();
        let write = self.db.begin_write()?;
        {
            let mut blobs = write.open_table(BLOBS)?;
            let mut access = write.open_table(ACCESS)?;
            for (hash, (payload, stamp)) in writes {
                if payload.len() as u64 + RECORD_BYTES > self.cap / 4 {
                    continue;
                }
                if let Some(entry) = self.index.remove(&hash) {
                    self.bytes -= entry.bytes + RECORD_BYTES;
                }
                let entry = Entry {
                    bytes: payload.len() as u64,
                    stamp,
                };
                blobs.insert(hash, payload.as_ref())?;
                access.insert(hash, (entry.bytes, entry.stamp))?;
                self.bytes += entry.bytes + RECORD_BYTES;
                self.index.insert(hash, entry);
            }
            for (hash, stamp) in touches {
                if let Some(entry) = self.index.get_mut(&hash) {
                    entry.stamp = entry.stamp.max(stamp);
                    access.insert(hash, (entry.bytes, entry.stamp))?;
                }
            }
            if self.bytes > self.cap / 4 || self.index.len() > MAX_DISK_ENTRIES {
                let mut victims = self
                    .index
                    .iter()
                    .map(|(&hash, entry)| (entry.stamp, hash))
                    .collect::<Vec<_>>();
                victims.sort_unstable();
                for (_, hash) in victims {
                    if self.bytes <= self.cap / 4 && self.index.len() <= MAX_DISK_ENTRIES {
                        break;
                    }
                    let entry = self.index.remove(&hash).expect("selected victim");
                    self.bytes -= entry.bytes + RECORD_BYTES;
                    blobs.remove(hash)?;
                    access.remove(hash)?;
                    evicted.push(hash);
                }
            }
        }
        write.commit()?;
        Ok(evicted)
    }

    /// Reclaims freed pages before growth uses the physical transaction headroom.
    pub(super) fn compact(&mut self) -> Result<()> {
        self.db.compact()?;
        Ok(())
    }
}
