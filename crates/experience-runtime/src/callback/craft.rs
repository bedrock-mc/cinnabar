//! Server WIT 0.6's world access: the server's crafting recipes and scheduled updates.

use anyhow::Result;

use super::{CallbackRes, Reach, stage};
use crate::host::cinnabar::experience_server::types::{Crafted, ItemKey, WorldError};
use crate::limits::{MAX_SCHEDULE_TICKS, MAX_SCHEDULED_UPDATES};
use crate::protocol::{BlockPos, Op};

impl CallbackRes {
    /// The first of the server's recipes that takes `grid`; see
    /// [`crate::protocol::Recipe::matches`].
    pub(crate) fn crafting_result(&mut self, grid: &[Option<ItemKey>]) -> Result<Option<Crafted>> {
        self.host_call()?;
        let cells: Vec<Option<(&str, u16)>> = grid
            .iter()
            .map(|cell| cell.as_ref().map(|key| (key.id.as_str(), key.metadata)))
            .collect();
        Ok(self
            .own
            .recipes
            .iter()
            .find(|recipe| recipe.matches(&cells))
            .map(|recipe| Crafted {
                id: recipe.output.id.clone(),
                metadata: recipe.output.metadata,
                count: recipe.output.count,
            }))
    }

    /// Stages an update of the owned block at `pos`, which `set-block-data` may write, `ticks`
    /// (1 to [`MAX_SCHEDULE_TICKS`]) ticks after the result commits; at most
    /// [`MAX_SCHEDULED_UPDATES`] a callback.
    pub(crate) fn schedule_update(
        &mut self,
        pos: BlockPos,
        ticks: u32,
    ) -> Result<Result<(), WorldError>> {
        self.host_call()?;
        if !(1..=MAX_SCHEDULE_TICKS).contains(&ticks) {
            return Ok(Err(WorldError::Denied));
        }
        let slot = match self.snapshot.write(pos, Reach::Network) {
            Ok(slot) => slot,
            Err(error) => return Ok(Err(error)),
        };
        if !slot.owned {
            return Ok(Err(WorldError::NotOwned));
        }
        let scheduled = self
            .ops
            .iter()
            .filter(|op| matches!(op, Op::ScheduleUpdate { .. }))
            .count();
        if scheduled == MAX_SCHEDULED_UPDATES {
            return Ok(Err(WorldError::QuotaExceeded));
        }
        stage(&mut self.ops, Op::ScheduleUpdate { pos, ticks })?;
        Ok(Ok(()))
    }
}
