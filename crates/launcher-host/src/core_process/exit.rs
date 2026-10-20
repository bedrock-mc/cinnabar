//! Constant-size exit evidence for the actual children owned by one Core guard.

use super::CoreStopOutcome;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Default)]
struct State {
    pending: usize,
    unconfirmed: bool,
}

/// Exit evidence stays separate from account, transport and presence selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoreExitState {
    /// At least one actual child or original reaper callback is still outstanding.
    Pending,
    /// Every adopted child exited and its original detached cleanup finished.
    Confirmed,
    /// A stop was unreaped or its original completion was lost; mutation remains blocked.
    Unconfirmed,
}

/// A readonly observer of one guard's actual child receipts, retained through replacements.
#[derive(Clone, Debug, Default)]
pub struct CoreExitBarrier(Arc<Mutex<State>>);

impl CoreExitBarrier {
    /// Returns actual exit evidence without waiting on or replacing the original reaper.
    pub fn state(&self) -> CoreExitState {
        let state = self.lock();
        if state.unconfirmed {
            CoreExitState::Unconfirmed
        } else if state.pending != 0 {
            CoreExitState::Pending
        } else {
            CoreExitState::Confirmed
        }
    }

    /// Creates exactly one receipt only when the same guard adopts an actual child owner.
    pub(super) fn started(&self) -> Receipt {
        let mut state = self.lock();
        state.pending = state
            .pending
            .checked_add(1)
            .expect("actual child count fits usize");
        Receipt(Some(self.clone()))
    }

    /// Poisoned exit accounting cannot become authority to mutate an account.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|error| {
            let mut state = error.into_inner();
            state.unconfirmed = true;
            state
        })
    }
}

#[derive(Debug)]
pub(super) struct Receipt(Option<CoreExitBarrier>);

impl Receipt {
    /// Consumes only this original child's receipt after its actual stop and cleanup.
    pub(super) fn finish(mut self, outcome: CoreStopOutcome) {
        if let Some(barrier) = self.0.take() {
            let mut state = barrier.lock();
            state.unconfirmed |= outcome == CoreStopOutcome::Unreaped;
            state.pending -= 1;
        }
    }
}

impl Drop for Receipt {
    fn drop(&mut self) {
        if let Some(barrier) = self.0.take() {
            let mut state = barrier.lock();
            state.unconfirmed = true;
            state.pending -= 1;
        }
    }
}
