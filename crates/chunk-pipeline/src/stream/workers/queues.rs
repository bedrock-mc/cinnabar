//! The world pool's queues: one per lane plus urgent work, and the order workers take
//! jobs in.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A lower lane's oldest job jumps the priority order after waiting this long, so
/// sustained mesh load cannot starve the decode and light work that mesh depends on.
pub(super) const DECODE_MAX_WAIT: Duration = Duration::from_millis(4);
pub(super) const LIGHT_MAX_WAIT: Duration = Duration::from_millis(16);

/// Work classes in scheduling order: mesh gates chunks appearing, decode feeds it, light trails,
/// and distant terrain sampling takes only what is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) enum Lane {
    Mesh,
    Decode,
    Light,
    Lod,
}

/// One queued job; light solves borrow the worker's scratch.
pub(super) type Job = Box<dyn FnOnce(&mut world::LightSolverScratch) + Send + 'static>;
/// One lane's jobs with the time each was queued, oldest first.
pub(super) type Queue = VecDeque<(Instant, Job)>;

/// Every lane's queue and the pool's shutdown flag, behind the pool's one lock.
#[derive(Default)]
pub(super) struct Queues {
    /// Work for a block the local player just changed, with its lane. Every worker takes it
    /// before any lane's backlog, so the change shows at once.
    pub(super) urgent: VecDeque<(Lane, Job)>,
    pub(super) mesh: Queue,
    pub(super) decode: Queue,
    pub(super) light: Queue,
    pub(super) lod: Queue,
    pub(super) shutdown: bool,
}

impl Queues {
    /// Queues `job` on `lane`, stamped with `queued_at`.
    pub(super) fn push(&mut self, lane: Lane, queued_at: Instant, job: Job) {
        let queue = match lane {
            Lane::Mesh => &mut self.mesh,
            Lane::Decode => &mut self.decode,
            Lane::Light => &mut self.light,
            Lane::Lod => &mut self.lod,
        };
        queue.push_back((queued_at, job));
    }

    /// Next job for a worker at time `now`; the clock is a parameter so ageing is testable.
    /// Urgent work comes first on any thread.
    pub(super) fn take(&mut self, background: bool, now: Instant) -> Option<Job> {
        if let Some((_, job)) = self.urgent.pop_front() {
            return Some(job);
        }
        let overdue = |queue: &Queue, limit| {
            queue
                .front()
                .is_some_and(|(queued, _)| now.saturating_duration_since(*queued) >= limit)
        };
        let lane = if background && overdue(&self.light, LIGHT_MAX_WAIT) {
            &mut self.light
        } else if overdue(&self.decode, DECODE_MAX_WAIT) || self.mesh.is_empty() {
            match (self.decode.is_empty() && background, self.light.is_empty()) {
                (true, false) => &mut self.light,
                (true, true) => &mut self.lod,
                (false, _) => &mut self.decode,
            }
        } else {
            &mut self.mesh
        };
        lane.pop_front().map(|(_, job)| job)
    }
}
