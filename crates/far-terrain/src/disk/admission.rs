//! Admission covers commands and their completed results until the owner consumes them.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::Command;

/// Disk work retained outside the resident summary store.
pub(super) const MAX_PENDING_BYTES: usize = 32 << 20;
pub(super) const MAX_PENDING_OPERATIONS: usize = 64;

#[derive(Default)]
pub(super) struct Admission {
    bytes: AtomicUsize,
    operations: AtomicUsize,
}

pub(super) struct Permit {
    admission: Arc<Admission>,
    bytes: usize,
}

pub(super) struct Envelope<T> {
    pub value: T,
    pub permit: Option<Permit>,
}

impl Admission {
    /// Reserves both payload bytes and an operation slot without waiting for the worker.
    pub fn acquire(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        self.operations
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                (count < MAX_PENDING_OPERATIONS).then_some(count + 1)
            })
            .ok()?;
        let mut permit = Permit {
            admission: Arc::clone(self),
            bytes: 0,
        };
        self.bytes
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes)
                    .filter(|total| *total <= MAX_PENDING_BYTES)
            })
            .ok()?;
        permit.bytes = bytes;
        Some(permit)
    }
}

impl Drop for Permit {
    /// Releases admission after work is discarded or its result is consumed.
    fn drop(&mut self) {
        self.admission
            .bytes
            .fetch_sub(self.bytes, Ordering::Relaxed);
        self.admission.operations.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Command {
    /// Charges canonical read space before IO, and actual retained write/probe payloads.
    pub(super) fn retained_bytes(&self) -> usize {
        let columns = |columns: &[(u16, crate::Versioned)]| {
            columns
                .iter()
                .map(|(_, column)| {
                    column.summary.bytes.len() + std::mem::size_of::<crate::Versioned>() + 32
                })
                .sum::<usize>()
        };
        let probes = |probes: &[(crate::ColumnPos, crate::ColumnSummary)]| {
            probes.len() * (crate::COLUMN_CELLS * std::mem::size_of::<crate::SurfaceCell>() + 64)
        };
        std::mem::size_of::<Self>()
            + match self {
                Self::Write { columns: data, .. } => columns(data),
                Self::Load { .. } => crate::region::MAX_REGION_BYTES * 3,
                Self::Resolve { probes: data, .. } => probes(data),
                Self::ResolveWrite {
                    columns: data,
                    probes: samples,
                    ..
                } => {
                    probes(samples)
                        + data
                            .iter()
                            .map(|(_, column)| {
                                column.summary.bytes.len()
                                    + std::mem::size_of::<crate::Versioned>()
                                    + 32
                            })
                            .sum::<usize>()
                }
                Self::ResolveFresh { .. } | Self::Sync(_) => 0,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paused producer/consumer cannot exceed either budget, and dropping work restores it.
    #[test]
    fn admission_stays_bounded_until_results_are_consumed() {
        let admission = Arc::new(Admission::default());
        let full = admission.acquire(MAX_PENDING_BYTES).unwrap();
        assert!(admission.acquire(1).is_none());
        drop(full);
        let slots: Vec<_> = (0..MAX_PENDING_OPERATIONS)
            .map(|_| admission.acquire(1).unwrap())
            .collect();
        assert!(admission.acquire(0).is_none());
        drop(slots);
        assert!(admission.acquire(MAX_PENDING_BYTES).is_some());
    }
}
