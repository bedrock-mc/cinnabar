//! Block-owner permission for retrying an already published unsent simulation tick.
use super::{BlockUseRuntime, LocalUse};
use crate::melee::SwingTracker;

impl BlockUseRuntime {
    /// Only this owner's Full-rejected swing may retry a published tick; nonswinging uses stay ready.
    pub fn may_attempt(&self, tick: u64, outcome: LocalUse, swings: &SwingTracker) -> bool {
        outcome == LocalUse::Nothing
            || !swings.tick_is_published(tick)
            || (self.rejected_tick == Some(tick)
                && self.position_authority == swings.authority_identity())
    }

    /// Retains permission only for a block swing actually refused by a full transport queue.
    pub fn refuse_transport(&mut self, tick: u64, outcome: LocalUse, full: bool) {
        self.rejected_tick = (full && outcome != LocalUse::Nothing).then_some(tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_use::RepeatClock;
    use crate::movement::LocalMovementEffectTimeline;
    use protocol::ItemUseTrigger;

    /// Produces a published unsent tick and an eligible fresh block-use press.
    fn fixture() -> (BlockUseRuntime, SwingTracker) {
        let mut runtime = BlockUseRuntime::default();
        runtime.synchronize((7, 1));
        runtime.observe_use(true, true, false, true);
        let mut swings = SwingTracker::default();
        swings.sync_ticks((7, 1), 101, &LocalMovementEffectTimeline::default());
        swings.published_progress(101);
        (runtime, swings)
    }

    #[test]
    fn fresh_swinging_block_use_waits_but_nothing_and_new_ticks_stay_ready() {
        let (runtime, swings) = fixture();
        assert!(!runtime.may_attempt(101, LocalUse::Place, &swings));
        assert!(!runtime.may_attempt(101, LocalUse::Interact, &swings));
        assert!(runtime.may_attempt(101, LocalUse::Nothing, &swings));
        assert!(runtime.may_attempt(102, LocalUse::Place, &swings));
    }

    #[test]
    fn only_own_full_rejection_qualifies_the_exact_tick_retry() {
        let (mut runtime, swings) = fixture();
        runtime.refuse_transport(101, LocalUse::Place, false);
        assert!(!runtime.may_attempt(101, LocalUse::Place, &swings));
        runtime.refuse_transport(101, LocalUse::Nothing, true);
        assert!(!runtime.may_attempt(101, LocalUse::Place, &swings));
        runtime.refuse_transport(101, LocalUse::Place, true);
        assert!(runtime.may_attempt(101, LocalUse::Place, &swings));
        assert!(!runtime.may_attempt(100, LocalUse::Place, &swings));
        let clock = RepeatClock {
            now_millis: 1_000,
            sneaking: false,
            speed: 0.0,
            survival: true,
        };
        assert!(!runtime.admit(
            ItemUseTrigger::PlayerInput,
            1_000,
            101,
            LocalUse::Place,
            clock,
            false
        ));
        assert!(runtime.may_attempt(101, LocalUse::Place, &swings));
        assert!(runtime.admit(
            ItemUseTrigger::PlayerInput,
            1_000,
            101,
            LocalUse::Place,
            clock,
            true
        ));
        assert!(!runtime.may_attempt(101, LocalUse::Place, &swings));
    }

    #[test]
    fn focus_attack_new_press_and_authority_changes_revoke_owned_retries() {
        for cancel in 0..6 {
            let (mut runtime, swings) = fixture();
            runtime.refuse_transport(101, LocalUse::Place, true);
            match cancel {
                0 => runtime.clear_press(),
                1 => {
                    runtime.observe_use(true, false, true, true);
                }
                2 => {
                    runtime.observe_use(true, true, false, true);
                }
                3 => runtime.synchronize((7, 2)),
                4 => runtime.synchronize((8, 1)),
                _ => {
                    runtime.stop_packets(42, false);
                }
            }
            assert!(!runtime.may_attempt(101, LocalUse::Place, &swings));
        }
    }

    #[test]
    fn a_released_latched_press_keeps_its_own_full_rejection_retry() {
        let (mut runtime, swings) = fixture();
        runtime.refuse_transport(101, LocalUse::Place, true);
        assert!(runtime.observe_use(false, false, false, true));
        assert!(runtime.may_attempt(101, LocalUse::Place, &swings));
        let clock = RepeatClock {
            now_millis: 1_000,
            sneaking: false,
            speed: 0.0,
            survival: true,
        };
        assert_eq!(
            runtime.due(false, 101, clock),
            Some((ItemUseTrigger::PlayerInput, 1_000))
        );
    }

    #[test]
    fn changing_the_selected_slot_revokes_its_owned_retry() {
        let (mut runtime, swings) = fixture();
        let stack = protocol::NetworkItemStack::empty();
        let item =
            protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
        let mut selection = crate::mining::FrozenMiningSelection { slot: 0, item };
        assert!(!runtime.selection_changed(&selection));
        runtime.refuse_transport(101, LocalUse::Place, true);
        selection.slot = 1;
        assert!(runtime.selection_changed(&selection));
        assert!(!runtime.may_attempt(101, LocalUse::Place, &swings));
    }

    #[test]
    fn owned_retry_cannot_cross_a_swing_authority_change() {
        let (mut runtime, mut swings) = fixture();
        runtime.refuse_transport(101, LocalUse::Place, true);
        swings.sync_ticks((7, 2), 101, &LocalMovementEffectTimeline::default());
        swings.published_progress(101);
        assert!(!runtime.may_attempt(101, LocalUse::Place, &swings));
    }

    #[test]
    fn foreign_deferred_swing_does_not_grant_a_fresh_block_retry() {
        let (runtime, mut swings) = fixture();
        let mut foreign = swings.clone();
        foreign.try_swing(102, client_world::ACTOR_SWING_TICKS);
        swings.defer_unadmitted_attempt(&foreign);
        swings.published_progress(102);
        assert!(!runtime.may_attempt(102, LocalUse::Place, &swings));
    }
}
