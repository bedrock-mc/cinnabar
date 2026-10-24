//! One FIFO for attack presses and the mining work admitted by those presses.

use std::collections::VecDeque;

use crate::{
    interaction_authority::MAX_PENDING_INTERACTION_MILLIS,
    survival_mining::{BlockBreakingAuthority, DestroyTarget, SurvivalTickPayload},
};

/// Retry ownership and the deadline of one press that has not been admitted.
#[derive(Debug, Default, Clone)]
pub(super) struct PendingPress {
    pub since: Option<u64>,
    pub rejected_tick: Option<u64>,
}

/// An admitted entry stays in front until its movement carrier enters the session FIFO.
#[derive(Debug, Clone)]
pub(super) enum Work {
    Pending(PendingPress),
    Block {
        tick: u64,
    },
    Start {
        tick: u64,
        target: Box<DestroyTarget>,
        authority: BlockBreakingAuthority,
    },
    Predicted {
        tick: u64,
        payload: Box<SurvivalTickPayload>,
    },
    /// A refused block press whose actions wait for their own unsent tick.
    Refused {
        tick: u64,
        payload: Box<SurvivalTickPayload>,
    },
    Carrying {
        tick: u64,
    },
}

/// Stable identity and current work for one user press.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub id: u64,
    pub work: Work,
}

/// Pending input and accepted mining payloads share one lifetime and ordering owner.
#[derive(Debug, Default, Clone)]
pub(super) struct AttackQueue {
    pub entries: VecDeque<Entry>,
    next_id: u64,
}

impl AttackQueue {
    /// Appends a distinct press without replacing an earlier waiting press.
    pub fn push(&mut self) -> u64 {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("attack press sequence exhausted");
        let id = self.next_id;
        self.entries.push_back(Entry {
            id,
            work: Work::Pending(PendingPress::default()),
        });
        id
    }

    /// Whether any press is still waiting, including those behind an accepted break.
    pub fn pending(&self) -> bool {
        self.first_pending().is_some()
    }

    /// The first unadmitted press supplies retry ownership for a proposed sample.
    pub fn first_pending(&self) -> Option<&PendingPress> {
        self.entries.iter().find_map(|entry| match &entry.work {
            Work::Pending(pending) => Some(pending),
            _ => None,
        })
    }

    /// Only the head can attempt packet admission.
    pub fn front_pending(&self) -> Option<&PendingPress> {
        match &self.entries.front()?.work {
            Work::Pending(pending) => Some(pending),
            _ => None,
        }
    }

    /// Mutates retry ownership only for the press currently at the FIFO head.
    pub fn front_pending_mut(&mut self) -> Option<&mut PendingPress> {
        match &mut self.entries.front_mut()?.work {
            Work::Pending(pending) => Some(pending),
            _ => None,
        }
    }

    /// Starts each pending entry's own wait and expires only entries past their deadline.
    pub fn expire(&mut self, now: u64) {
        self.entries.retain_mut(|entry| match &mut entry.work {
            Work::Pending(pending) => {
                let since = *pending.since.get_or_insert(now);
                now.saturating_sub(since) <= MAX_PENDING_INTERACTION_MILLIS
            }
            _ => true,
        });
    }

    /// Cancels unadmitted input while preserving previously admitted starts and predictions.
    pub fn cancel_pending(&mut self) {
        self.entries
            .retain(|entry| !matches!(entry.work, Work::Pending(_)));
    }
}
