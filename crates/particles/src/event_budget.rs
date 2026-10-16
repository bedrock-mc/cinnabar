//! Shared admission for one simulation step's authored event work and temporary requests.

use crate::system::{MAX_EMITTERS, MAX_LIVE_PARTICLES};

/// Undrained sound requests retain the newest entries.
pub(crate) const MAX_QUEUED_SOUNDS: usize = 256;

/// Cumulative event admission losses, independent of retained live-particle limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventOverflows {
    /// Refused work units and unvisited sibling roots, not every descendant of a skipped tree.
    pub work: u64,
    /// Deferred event candidates skipped before scheduling or dispatch.
    pub deferred: u64,
    /// New nested-emitter requests refused before constructing their owned payload.
    pub spawns: u64,
    /// Old sound requests displaced, or new sounds refused by a zero-capacity queue.
    pub sounds: u64,
}

impl EventOverflows {
    /// Adds diagnostic counts without allowing long-running sessions to wrap them.
    pub(crate) fn add(&mut self, other: Self) {
        self.work = self.work.saturating_add(other.work);
        self.deferred = self.deferred.saturating_add(other.deferred);
        self.spawns = self.spawns.saturating_add(other.spawns);
        self.sounds = self.sounds.saturating_add(other.sounds);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EventLimits {
    pub work: usize,
    pub deferred: usize,
    pub spawns: usize,
    pub sounds: usize,
}

impl Default for EventLimits {
    /// Bounds event amplification in proportion to existing retained-state limits.
    fn default() -> Self {
        Self {
            work: 8 * MAX_LIVE_PARTICLES,
            deferred: MAX_LIVE_PARTICLES,
            spawns: MAX_EMITTERS,
            sounds: MAX_QUEUED_SOUNDS,
        }
    }
}

pub(crate) struct EventBudget {
    pub limits: EventLimits,
    remaining: usize,
    deferred: usize,
    pub overflow: EventOverflows,
}

impl Default for EventBudget {
    fn default() -> Self {
        Self::new(EventLimits::default())
    }
}

impl EventBudget {
    /// Starts an independent allowance; Outputs shares it across all emitters in a step.
    pub fn new(limits: EventLimits) -> Self {
        Self {
            limits,
            remaining: limits.work,
            deferred: 0,
            overflow: EventOverflows::default(),
        }
    }

    /// Reserves work before traversal, lookup or expression evaluation; refusal ends the allowance.
    pub fn charge(&mut self, units: usize) -> bool {
        if units > self.remaining {
            self.remaining = 0;
            self.skip_work(units);
            false
        } else {
            self.remaining -= units;
            true
        }
    }

    /// Counts omitted sibling roots without walking their descendants.
    pub fn skip_work(&mut self, roots: usize) {
        self.overflow.work = self.overflow.work.saturating_add(roots as u64);
    }

    /// Counts deferred candidates omitted from this update, without walking skipped definitions.
    pub fn skip_deferred(&mut self, count: usize) {
        self.overflow.deferred = self.overflow.deferred.saturating_add(count as u64);
    }

    /// Reserves one deferred entry before a pending vector can grow.
    pub fn defer(&mut self) -> bool {
        if !self.charge(1) || self.deferred >= self.limits.deferred {
            self.skip_deferred(1);
            false
        } else {
            self.deferred += 1;
            true
        }
    }
}
