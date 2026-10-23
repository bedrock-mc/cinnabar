//! Granted, synchronous action-filter entry points.
use crate::{ActionFrame, ActionVerdict, ModHost};

impl ModHost {
    /// Stable identity of this opt-in, granted filter; replacements get a fresh identity.
    pub fn action_filter_identity(&self) -> Option<u64> {
        self.instance.action_filter_identity()
    }

    /// Runs the return-value-only input callback without sharing ordinary frame fuel.
    pub fn filter_actions(&mut self, frame: ActionFrame) -> anyhow::Result<ActionVerdict> {
        self.instance.filter_actions(frame)
    }
}
