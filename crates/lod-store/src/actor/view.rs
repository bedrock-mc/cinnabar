//! What read-through reads show other than by writes: session changes, leading epoch
//! changes and the regions an undecided session stopped reading through, as clients see
//! them.

use std::{
    collections::VecDeque,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use lod::epoch::RegionPos;

use crate::identity::Dimension;

/// Hidden regions clients can still ask for by [`ViewEpoch`]; a client further behind
/// reads everything again.
pub const KEPT_HIDDEN_REGIONS: usize = 1024;

/// How often what [`super::ReadTarget::Section`] reads show changed other than by writes:
/// every session change, every change of an undecided session's leading epoch, and every
/// region an undecided session stopped reading through to a stored epoch. Every count
/// only grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ViewEpoch {
    /// Session changes queued or decided.
    pub sessions: u64,
    /// Leading epoch changes of undecided sessions since the store opened; see
    /// [`crate::store::Store::leading_epoch_changes`].
    pub leading: u64,
    /// Regions hidden from read-through since the store opened.
    pub hidden: u64,
}

/// What changed in read-through reads between two [`ViewEpoch`]s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewChange {
    /// Nothing.
    Unchanged,
    /// Only these regions of these dimensions stopped reading through, oldest first;
    /// sections overlapping them at any level read differently.
    Hidden(Vec<(Dimension, RegionPos)>),
    /// A session or a leading epoch changed, or more regions were hidden than are kept:
    /// anything may read differently.
    All,
}

/// The regions hidden so far: their count and the latest [`KEPT_HIDDEN_REGIONS`].
#[derive(Debug, Default)]
struct HiddenLog {
    total: u64,
    recent: VecDeque<(Dimension, RegionPos)>,
}

impl HiddenLog {
    /// The regions hidden after the first `seen`; `None` once some were dropped.
    fn since(&self, seen: u64) -> Option<Vec<(Dimension, RegionPos)>> {
        let first_kept = self.total - self.recent.len() as u64;
        let skip = seen.checked_sub(first_kept)?;
        Some(
            self.recent
                .iter()
                .skip(usize::try_from(skip).unwrap_or(usize::MAX))
                .copied()
                .collect(),
        )
    }
}

/// Hidden regions and leading epoch changes the store thread publishes and clients read;
/// the counts are readable without the lock, and the hidden count moves on only once the
/// regions are logged.
#[derive(Debug, Default)]
pub(super) struct HiddenRegions {
    count: AtomicU64,
    leading: AtomicU64,
    log: Mutex<HiddenLog>,
}

impl HiddenRegions {
    /// Logs `regions`, newly hidden, then publishes their count. The store thread calls
    /// it before answering reads that no longer see them.
    pub(super) fn publish(&self, regions: impl IntoIterator<Item = (Dimension, RegionPos)>) {
        let mut log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        for region in regions {
            log.total += 1;
            log.recent.push_back(region);
            if log.recent.len() > KEPT_HIDDEN_REGIONS {
                log.recent.pop_front();
            }
        }
        self.count.store(log.total, Ordering::Release);
    }

    /// Publishes the store's leading epoch change count. The store thread calls it before
    /// answering reads that see the new leading epoch.
    pub(super) fn publish_leading(&self, changes: u64) {
        self.leading.store(changes, Ordering::Release);
    }

    /// The published hidden region count.
    pub(super) fn count(&self) -> u64 {
        self.count.load(Ordering::Acquire)
    }

    /// The published leading epoch change count.
    pub(super) fn leading(&self) -> u64 {
        self.leading.load(Ordering::Acquire)
    }

    /// The view epoch with `sessions` session changes, and what changed since `since`.
    pub(super) fn changes(&self, sessions: u64, since: ViewEpoch) -> (ViewEpoch, ViewChange) {
        let log = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        let now = ViewEpoch {
            sessions,
            leading: self.leading(),
            hidden: log.total,
        };
        let change = if sessions != since.sessions || now.leading != since.leading {
            ViewChange::All
        } else if log.total == since.hidden {
            ViewChange::Unchanged
        } else {
            log.since(since.hidden)
                .map_or(ViewChange::All, ViewChange::Hidden)
        };
        (now, change)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OVERWORLD: Dimension = Dimension(0);

    fn region(x: i32) -> (Dimension, RegionPos) {
        (OVERWORLD, RegionPos { x, z: 0 })
    }

    #[test]
    fn clients_get_the_regions_hidden_since_their_epoch_or_everything() {
        let hidden = HiddenRegions::default();
        let start = ViewEpoch::default();
        assert_eq!(hidden.changes(0, start).1, ViewChange::Unchanged);
        hidden.publish([region(1), region(2)]);
        assert_eq!(hidden.count(), 2);
        let (seen, change) = hidden.changes(0, start);
        assert_eq!(change, ViewChange::Hidden(vec![region(1), region(2)]));
        hidden.publish([region(3)]);
        assert_eq!(
            hidden.changes(0, seen).1,
            ViewChange::Hidden(vec![region(3)])
        );
        assert_eq!(
            hidden.changes(1, seen).1,
            ViewChange::All,
            "a session changed"
        );
        hidden.publish_leading(1);
        let (led, change) = hidden.changes(0, seen);
        assert_eq!(change, ViewChange::All, "a leading epoch changed");
        assert_eq!(hidden.changes(0, led).1, ViewChange::Unchanged);
        hidden.publish((0..KEPT_HIDDEN_REGIONS as i32).map(region));
        assert_eq!(hidden.changes(0, seen).1, ViewChange::All, "fell behind");
    }
}
