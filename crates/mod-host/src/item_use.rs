use super::{MAX_IMPORT_WRITES, State, cinnabar};
use crate::GameplaySnapshot;
use anyhow::{Result, bail};

/// Only a successful callback may publish policy for its current world scope.
#[derive(Default)]
pub(super) struct ItemUsePolicy {
    pending: bool,
    writes: u32,
    pub committed: Option<(u64, i32)>,
}

impl ItemUsePolicy {
    pub fn commit(&mut self, snapshot: Option<&GameplaySnapshot>) {
        self.committed = snapshot
            .filter(|_| self.pending)
            .map(|frame| (frame.session, frame.dimension));
    }
}

impl cinnabar::extension::item_use::Host for State {
    fn set_delay_fix(&mut self, enabled: bool) -> Result<Result<(), String>> {
        self.item_use_policy.writes += 1;
        if self.item_use_policy.writes > MAX_IMPORT_WRITES {
            bail!("item-use import budget exhausted");
        }
        if !self.grants.item_use {
            return Ok(Err("item-use capability denied".into()));
        }
        self.item_use_policy.pending = enabled;
        Ok(Ok(()))
    }
}
