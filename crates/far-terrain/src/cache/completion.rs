//! Ordered, resumable application of disk results on the cache owner.

use std::ops::Bound::{Excluded, Unbounded};

use super::*;
use crate::{config::CACHE_EVENTS_PER_UPDATE, disk::DiskEvent};

pub(super) enum PendingEvent {
    Loaded {
        namespace: NamespaceKey,
        region: RegionPos,
        columns: std::vec::IntoIter<(ColumnPos, Versioned)>,
        after: Option<ColumnPos>,
    },
    Written {
        namespace: NamespaceKey,
        columns: std::vec::IntoIter<(ColumnPos, u64)>,
    },
}

impl FarTerrainCache {
    /// Starts only a bounded number of events and finishes each before reading another.
    pub(super) fn apply_completions(&mut self, now: Instant, budget: &mut SummaryBudget) {
        for _ in 0..CACHE_EVENTS_PER_UPDATE {
            if let Some(namespace) = self.namespace {
                while let Some((_, column)) = self.unresolved.last() {
                    if !budget.take(column.summary.bytes.len()) {
                        return;
                    }
                    let (pos, column) = self.unresolved.pop().expect("checked above");
                    self.queue_write(namespace, pos, column, now);
                }
            }
            if let Some(mut pending) = self.pending_event.take()
                && !self.apply_event(&mut pending, budget)
            {
                self.pending_event = Some(pending);
                return;
            }
            let Some(event) = self.disk.as_ref().and_then(|disk| disk.events().next()) else {
                return;
            };
            match event {
                DiskEvent::Resolved {
                    world,
                    entry,
                    epoch,
                } => {
                    if entry == self.entry {
                        self.resolved(world, epoch);
                    }
                }
                DiskEvent::Loaded {
                    namespace,
                    region,
                    columns,
                } => {
                    self.pending_event = Some(PendingEvent::Loaded {
                        namespace,
                        region,
                        columns: columns.into_iter(),
                        after: None,
                    });
                }
                DiskEvent::Written { namespace, columns } => {
                    self.pending_event = Some(PendingEvent::Written {
                        namespace,
                        columns: columns.into_iter(),
                    });
                }
            }
        }
    }

    /// Applies columns individually, including queued versions absent from a region read.
    fn apply_event(&mut self, pending: &mut PendingEvent, budget: &mut SummaryBudget) -> bool {
        match pending {
            PendingEvent::Loaded {
                namespace,
                region,
                columns,
                after,
            } => {
                while let Some((pos, disk)) = columns.as_slice().first() {
                    let queued = (self.namespace == Some(*namespace))
                        .then(|| self.queued(*namespace, *pos))
                        .flatten();
                    let newest = queued
                        .filter(|queued| queued.version > disk.version)
                        .unwrap_or(disk);
                    if !budget.take(disk.summary.bytes.len().max(newest.summary.bytes.len())) {
                        return false;
                    }
                    let newest = newest.clone();
                    let (pos, _) = columns.next().expect("checked above");
                    if self.namespace == Some(*namespace) {
                        self.store.admit_versioned(pos, newest);
                    }
                }
                if self.namespace != Some(*namespace) {
                    return true;
                }
                loop {
                    let bounds = (after.map_or(Unbounded, Excluded), Unbounded);
                    let key = (*namespace, *region);
                    let waiting = self
                        .dirty
                        .get(&key)
                        .and_then(|dirty| dirty.columns.range(bounds).next());
                    let writing = self
                        .writing
                        .get(&key)
                        .and_then(|columns| columns.range(bounds).next());
                    let Some(pos) = waiting
                        .into_iter()
                        .chain(writing)
                        .map(|(pos, _)| *pos)
                        .min()
                    else {
                        return true;
                    };
                    let newest = self.queued(*namespace, pos).expect("queued column exists");
                    if !budget.take(newest.summary.bytes.len()) {
                        return false;
                    }
                    let newest = newest.clone();
                    self.store.admit_versioned(pos, newest);
                    *after = Some(pos);
                }
            }
            PendingEvent::Written { namespace, columns } => {
                while let Some((pos, _)) = columns.as_slice().first() {
                    let bytes = self
                        .writing
                        .get(&(*namespace, RegionPos::of(*pos)))
                        .and_then(|columns| columns.get(pos))
                        .map_or(0, |column| column.summary.bytes.len());
                    if !budget.take(bytes) {
                        return false;
                    }
                    let (pos, version) = columns.next().expect("checked above");
                    let key = (*namespace, RegionPos::of(pos));
                    if let Some(writing) = self.writing.get_mut(&key) {
                        if writing
                            .get(&pos)
                            .is_some_and(|column| column.version <= version)
                        {
                            writing.remove(&pos);
                        }
                        if writing.is_empty() {
                            self.writing.remove(&key);
                        }
                    }
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests;
