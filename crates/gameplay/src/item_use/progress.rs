//! Tick-bound progress for an accepted eating or drinking use.
use super::*;
use assets::facts::ItemUseAnimationFact;

impl ItemUseRuntime {
    /// Sends due progress before its input, retaining successful admission across input retries.
    pub fn send_progress_tick<E>(
        &mut self,
        tick: u64,
        local_runtime_id: u64,
        selection: Option<&FrozenMiningSelection>,
        animation: Option<ItemUseAnimationFact>,
        send: impl FnOnce(protocol::Packet) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.release_pending
            || !matches!(
                animation,
                Some(ItemUseAnimationFact::Eat | ItemUseAnimationFact::Drink)
            )
        {
            return Ok(());
        }
        let Some(active) = self.active.as_mut() else {
            return Ok(());
        };
        let Some(selection) = selection else {
            return Ok(());
        };
        if active.selection_changed(Some(selection)) || selection.item.is_empty() {
            self.active = None;
            return Ok(());
        }
        if active.last_progress_tick.is_some_and(|last| tick <= last)
            || selection.item.block_runtime_id() != 0
        {
            return Ok(());
        }
        let Some(elapsed) = tick
            .checked_sub(active.started_tick)
            .and_then(|ticks| ticks.checked_add(1))
        else {
            return Ok(());
        };
        let Some(remaining) = u64::from(active.max_ticks).checked_sub(elapsed) else {
            return Ok(());
        };
        if remaining >= 26 || remaining % 4 != 0 {
            return Ok(());
        }
        let Ok(item_id) = i16::try_from(selection.item.network_id()) else {
            return Ok(());
        };
        let auxiliary = (selection.item.metadata() as i16).max(0);
        send(protocol::item_use_progress_packet(
            local_runtime_id,
            item_id,
            auxiliary,
        ))?;
        active.last_progress_tick = Some(tick);
        Ok(())
    }
}
