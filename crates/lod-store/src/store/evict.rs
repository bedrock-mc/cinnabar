//! Keeping a store inside its bounds: the per-store epoch limit and far-region eviction
//! under a byte cap.

use std::{fs, io};

use super::{Store, StoreEvent};
use crate::{
    access::RegionRef, epochs::EpochRef, error::StoreError, identity::Dimension,
    region_file::Access, region_id::RegionId, signatures::signature_file_name,
};

/// Where the player is, so eviction removes far regions before near ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Focus {
    /// The dimension the player is in; regions of other dimensions go first.
    pub dimension: Dimension,
    /// The player's block x.
    pub block_x: i64,
    /// The player's block z.
    pub block_z: i64,
}

/// A region file eviction may delete.
#[derive(Debug)]
struct Victim {
    region: RegionRef,
    bytes: u64,
    /// Eviction order: regions the player cannot see first (other dimensions, epochs no
    /// session uses, or every region without a focus), least recently accessed first;
    /// then the focus epochs' regions, farthest first, so a near region that stays
    /// resident without being read again is never taken before a far one. Ties go to
    /// the older access, then the finest level, which frees the most detail per covered
    /// area.
    rank: (bool, u64, u32, u8),
}

impl Store {
    /// Deletes least recently used established epochs, outside those the open sessions
    /// use or vote against, while more than [`crate::epochs::MAX_EPOCHS_PER_STORE`] exist.
    pub(super) fn enforce_epoch_limit(&mut self) {
        let keep: Vec<EpochRef> = self.epochs_in_use();
        while let Some(epoch) = self.catalog.over_limit(&keep) {
            self.discard_epoch(epoch);
            self.events.push(StoreEvent::EpochEvicted(epoch));
        }
    }

    /// Deletes region files until the store holds at most `cap` bytes: first those
    /// `focus` cannot see, least recently used first, then those of the focus
    /// dimension's session epochs, farthest from `focus` first. A level-0 region takes
    /// its signature file with it. Returns the bytes freed; a read-only store frees none.
    pub fn evict_regions(&mut self, cap: u64, focus: Option<Focus>) -> Result<u64, StoreError> {
        let used = self.disk_bytes()?;
        self.evict_regions_from(used, cap, focus)
    }

    /// [`Self::evict_regions`] for a store the caller just measured at `used` bytes.
    pub(crate) fn evict_regions_from(
        &mut self,
        mut used: u64,
        cap: u64,
        focus: Option<Focus>,
    ) -> Result<u64, StoreError> {
        if self.access == Access::ReadOnly || used <= cap {
            return Ok(0);
        }
        let mut victims = self.victims(focus)?;
        victims.sort_unstable_by_key(|victim| victim.rank);
        let mut freed = 0;
        for victim in victims {
            if used <= cap {
                break;
            }
            let RegionRef { epoch, region } = victim.region;
            let mut bytes = self.files(epoch)?.regions.delete_region(region)?;
            if region.level() == 0 {
                bytes += self.files(epoch)?.signatures.delete(region)?;
            }
            self.table.forget(victim.region);
            self.table_dirty = true;
            used = used.saturating_sub(bytes);
            freed += bytes;
            self.events.push(StoreEvent::RegionEvicted {
                region: victim.region,
                bytes: victim.bytes,
            });
        }
        Ok(freed)
    }

    /// Every region file of every catalogued epoch, with its size and eviction rank.
    fn victims(&self, focus: Option<Focus>) -> io::Result<Vec<Victim>> {
        let in_use = self.epochs_in_use();
        let mut victims = Vec::new();
        for entry in self.catalog.entries() {
            let dir = entry.epoch.dir(&self.dir);
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            for file in entries {
                let file = file?;
                let Some(region) = file.file_name().to_str().and_then(RegionId::from_file_name)
                else {
                    continue;
                };
                let mut bytes = file.metadata()?.len();
                if region.level() == 0 {
                    bytes += fs::metadata(dir.join(signature_file_name(region)))
                        .map_or(0, |metadata| metadata.len());
                }
                let region = RegionRef {
                    epoch: entry.epoch,
                    region,
                };
                let minute = self.table.region(region).get();
                let visible = focus.filter(|focus| {
                    focus.dimension == entry.epoch.dimension && in_use.contains(&entry.epoch)
                });
                let order = match visible {
                    Some(focus) => u64::MAX - distance(region.region, focus),
                    None => u64::from(minute),
                };
                victims.push(Victim {
                    region,
                    bytes,
                    rank: (visible.is_some(), order, minute, region.region.level()),
                });
            }
        }
        Ok(victims)
    }
}

/// Squared horizontal distance in blocks from `focus` to the centre of `region`.
fn distance(region: RegionId, focus: Focus) -> u64 {
    let [x, z] = region.center_block();
    let (dx, dz) = (x.abs_diff(focus.block_x), z.abs_diff(focus.block_z));
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
}
