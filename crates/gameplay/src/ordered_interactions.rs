//! One bounded admission cursor for mutating input, including accepted carrier fences.

use semantic_input::{
    MAX_PENDING_ACTION_EVENTS,
    identity::{InputAuthorityGeneration, InputSequence},
};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use std::collections::VecDeque;

/// Authorities that may invalidate work which has not entered the outbound FIFO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionScope {
    pub session: SessionGeneration,
    pub position: PositionAuthorityGeneration,
    pub dimension: i32,
    pub input: InputAuthorityGeneration,
}

/// World replacement retires old carriers; input cancellation preserves accepted work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalidation {
    None,
    Input,
    World,
}

/// One event and its private attempt state, retained at the head on backpressure.
#[derive(Debug)]
pub struct PendingInteraction<T> {
    pub value: T,
    pub since_millis: u64,
    /// Accepted work cannot be discarded while its movement or release carrier waits.
    pub admitted: bool,
}

/// Admission state has no clock or transport access; adapters supply both.
#[derive(Debug)]
pub struct OrderedInteractions<T> {
    entries: VecDeque<PendingInteraction<T>>,
    scope: Option<InteractionScope>,
    frame: Option<(InteractionScope, InputSequence)>,
    quarantined: bool,
}

impl<T> Default for OrderedInteractions<T> {
    /// Starts with no pending input and no observed authority.
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            scope: None,
            frame: None,
            quarantined: false,
        }
    }
}

impl<T> OrderedInteractions<T> {
    /// Synchronizes lifetime once per finalized frame; returns whether input was cancelled.
    pub fn synchronize(&mut self, scope: InteractionScope, invalidated: bool) -> Invalidation {
        let world_changed = self.scope.is_some_and(|previous| {
            (previous.session, previous.position, previous.dimension)
                != (scope.session, scope.position, scope.dimension)
        });
        let changed = self.scope.is_some_and(|previous| previous != scope);
        if world_changed {
            self.entries.clear();
        } else if changed || invalidated {
            self.cancel_pending();
        }
        self.scope = Some(scope);
        if world_changed {
            Invalidation::World
        } else if changed || invalidated {
            Invalidation::Input
        } else {
            Invalidation::None
        }
    }

    /// Claims an input frame once, preventing repeated producer passes from replaying edges.
    pub fn observe_frame(&mut self, scope: InteractionScope, frame: InputSequence) -> bool {
        if self.frame == Some((scope, frame)) {
            return false;
        }
        self.frame = Some((scope, frame));
        true
    }

    /// Drops only unadmitted input; accepted mining/release fences remain in the FIFO.
    pub fn cancel_pending(&mut self) {
        self.entries.retain(|entry| entry.admitted);
    }

    /// Stops admitting input after an unknown gap without evicting accepted carriers.
    pub fn quarantine(&mut self) {
        self.cancel_pending();
        self.quarantined = true;
    }

    /// Rearms only after a complete neutral sample, so a missing release cannot stick.
    pub fn observe_neutral(&mut self, neutral: bool) {
        if neutral {
            self.quarantined = false;
        }
    }

    /// Whether a gap still prevents new gestures from entering the queue.
    pub const fn quarantined(&self) -> bool {
        self.quarantined
    }

    /// Appends a complete event, returning it on overflow and quarantining the missing suffix.
    pub fn push(&mut self, value: T, now_millis: u64) -> Result<(), T> {
        if self.quarantined || self.entries.len() == MAX_PENDING_ACTION_EVENTS {
            self.quarantine();
            return Err(value);
        }
        self.entries.push_back(PendingInteraction {
            value,
            since_millis: now_millis,
            admitted: false,
        });
        Ok(())
    }

    /// Moves out only the head while its adapter attempts admission.
    pub fn pop_front(&mut self) -> Option<PendingInteraction<T>> {
        self.entries.pop_front()
    }

    /// Restores the same failed head, preserving its captured context and original deadline.
    pub fn restore_front(&mut self, entry: PendingInteraction<T>) {
        assert!(
            self.entries.len() < MAX_PENDING_ACTION_EVENTS,
            "restored interaction must own a reserved queue slot"
        );
        self.entries.push_front(entry);
    }

    /// Whether any action or accepted carrier still owns the admission cursor.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<T> PendingInteraction<T> {
    /// Pending input expires on the caller's clock; accepted carrier work never expires.
    pub fn expired(&self, now_millis: u64) -> bool {
        !self.admitted
            && now_millis.saturating_sub(self.since_millis)
                > crate::interaction_authority::MAX_PENDING_INTERACTION_MILLIS
    }
}

#[cfg(test)]
mod tests;
