//! Process-wide bounds for copied work and undelivered results.

use std::{
    mem::size_of,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use far_terrain::{SurfaceCell, SurfaceScan, SurfaceSlice};

/// One policy for all active and retiring sessions. Source snapshots and immutable asset
/// registries stay with producers; workers receive bounded copies of relevant terrain only.
/// Reservations include closure/context handles, scan/output storage, compressed-result
/// headroom and copied palettes/biomes. They cover queued, running and undrained work.
/// Algorithm-local temporary scratch is not retained across jobs. Shared source registries
/// (including prepared colours) are not claimed to fit this copied-work budget.
pub(super) const POLICY: Policy = Policy {
    jobs: 4,
    bytes: 1024 * 1024,
    slice_bytes: 256 * 1024,
};

#[derive(Clone, Copy)]
pub(super) struct Policy {
    pub(super) jobs: usize,
    pub(super) bytes: usize,
    pub(super) slice_bytes: usize,
}

/// Fixed reservation also covers encoder output (at most a stored deflate block per plane),
/// channel nodes, Arc headers and context/continuation metadata, with conservative padding.
pub(super) const JOB_OVERHEAD: usize =
    size_of::<SurfaceScan>() + 4 * size_of::<SurfaceCell>() * far_terrain::COLUMN_CELLS + 4096;

const _: () = assert!(POLICY.slice_bytes >= SurfaceSlice::MIN_BYTES);
const _: () = assert!(POLICY.bytes >= POLICY.slice_bytes + JOB_OVERHEAD);

#[derive(Default, Debug, Clone, Copy)]
pub(super) struct Usage {
    pub(super) jobs: usize,
    pub(super) bytes: usize,
}

pub(super) struct Admission {
    policy: Policy,
    usage: Mutex<Usage>,
    waiting: Mutex<Vec<Weak<Wake>>>,
}

pub(super) static GLOBAL: LazyLock<Arc<Admission>> =
    LazyLock::new(|| Arc::new(Admission::new(POLICY)));

impl Admission {
    /// Creates an independent owner for deterministic tests; production uses only GLOBAL.
    pub(super) fn new(policy: Policy) -> Self {
        Self {
            policy,
            usage: Mutex::default(),
            waiting: Mutex::default(),
        }
    }

    /// Wakes a live inbox when another producer releases global capacity.
    pub(super) fn subscribe(&self, tx: crossbeam_channel::Sender<super::Event>) -> Arc<Wake> {
        let wake = Arc::new(Wake {
            tx,
            signalled: AtomicBool::new(false),
        });
        let mut waiting = self
            .waiting
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        waiting.retain(|entry| entry.strong_count() != 0);
        waiting.push(Arc::downgrade(&wake));
        wake
    }

    /// Atomically reserves count and bytes before any payload leaves a producer.
    pub(super) fn acquire(self: &Arc<Self>, bytes: usize) -> Option<Permit> {
        let mut usage = self
            .usage
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if usage.jobs >= self.policy.jobs || bytes > self.policy.bytes.saturating_sub(usage.bytes) {
            return None;
        }
        usage.jobs += 1;
        usage.bytes += bytes;
        Some(Permit {
            owner: Arc::clone(self),
            bytes,
        })
    }

    /// Largest compact slice that can be admitted by this owner.
    pub(super) fn slice_bytes(&self) -> usize {
        self.policy.slice_bytes
    }

    /// Exposes the fixture's actual limits without duplicating policy in assertions.
    #[cfg(any(test, feature = "publication-test-support"))]
    pub(super) fn policy(&self) -> Policy {
        self.policy
    }

    /// Observes reservations without clocks or worker execution.
    #[cfg(any(test, feature = "publication-test-support"))]
    pub(super) fn usage(&self) -> Usage {
        *self
            .usage
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// Ownership follows a job into its result, including a deferred or stale result.
pub(super) struct Permit {
    owner: Arc<Admission>,
    bytes: usize,
}

impl Permit {
    /// Returns unused conservative input reservation after the exact compact copy is known.
    pub(super) fn shrink(&mut self, bytes: usize) {
        assert!(bytes <= self.bytes);
        let mut usage = self
            .owner
            .usage
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        usage.bytes -= self.bytes - bytes;
        self.bytes = bytes;
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut usage = self
            .owner
            .usage
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        usage.jobs -= 1;
        usage.bytes -= self.bytes;
        drop(usage);
        let mut waiting = self
            .owner
            .waiting
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        waiting.retain(|entry| {
            let Some(wake) = entry.upgrade() else {
                return false;
            };
            if !wake.signalled.swap(true, Ordering::AcqRel) {
                super::workers::send_result(&wake.tx, super::Event::Prepared);
            }
            true
        });
    }
}

/// Coalesces capacity notices so an idle receiver cannot accumulate an unbounded notice queue.
pub(super) struct Wake {
    tx: crossbeam_channel::Sender<super::Event>,
    signalled: AtomicBool,
}

impl Wake {
    /// Allows the next capacity transition to notify this inbox again.
    pub(super) fn acknowledge(&self) {
        self.signalled.store(false, Ordering::Release);
    }
}
