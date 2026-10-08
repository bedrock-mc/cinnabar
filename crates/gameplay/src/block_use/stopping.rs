use super::BlockUseRuntime;

impl BlockUseRuntime {
    /// A refused stop must leave before another placement can start.
    pub fn stopping(&self) -> bool {
        self.stopping
    }

    /// Retains the stop destination and any new press until transport accepts the stop.
    pub fn stop_packets(&mut self, local_runtime_id: u64, repress: bool) -> Vec<protocol::Packet> {
        self.rejected_tick = None;
        self.stop_repress |= repress;
        let Some(destination) = self.last_success_destination() else {
            return Vec::new();
        };
        self.stopping = true;
        vec![protocol::stop_item_use_on_packet(
            local_runtime_id,
            destination,
        )]
    }

    /// Clears an admitted hold while preserving a press received during a refused stop.
    pub fn admit_stop(&mut self, admitted: bool) -> bool {
        if admitted {
            let repress = self.stop_repress;
            self.clear();
            self.latched_press = repress;
        }
        admitted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_use::{LocalUse, RepeatClock};
    use protocol::ItemUseTrigger;

    #[test]
    fn refused_stop_preserves_destination_and_replays_a_quick_press_after_admission() {
        let mut runtime = BlockUseRuntime::default();
        runtime.intention.record(
            false,
            [0, 63, 1],
            LocalUse::Place,
            true,
            false,
            [0.5, 64.0, 0.5],
        );
        assert_eq!(runtime.stop_packets(42, true).len(), 1);
        assert!(!runtime.admit_stop(false));
        assert!(runtime.stopping());
        assert_eq!(runtime.last_success_destination(), Some([0, 63, 1]));
        assert_eq!(runtime.stop_packets(42, false).len(), 1);
        assert!(runtime.admit_stop(true));
        assert!(!runtime.stopping());
        assert_eq!(runtime.last_success_destination(), None);
        assert!(runtime.observe_use(false, false, false, true));
        let clock = RepeatClock {
            now_millis: 1_000,
            sneaking: false,
            speed: 0.0,
            survival: true,
        };
        assert_eq!(
            runtime.due(false, 1, clock),
            Some((ItemUseTrigger::PlayerInput, 1_000))
        );
    }

    #[test]
    fn blocked_input_cancels_a_deferred_press_without_discarding_its_refused_stop() {
        let mut runtime = BlockUseRuntime::default();
        runtime.intention.record(
            false,
            [0, 63, 1],
            LocalUse::Place,
            true,
            false,
            [0.5, 64.0, 0.5],
        );
        assert_eq!(runtime.stop_packets(42, true).len(), 1);
        assert!(!runtime.admit_stop(false));
        runtime.clear_press();
        assert!(runtime.stopping());
        assert_eq!(runtime.last_success_destination(), Some([0, 63, 1]));
        assert_eq!(runtime.stop_packets(42, false).len(), 1);
        assert!(runtime.admit_stop(true));
        assert!(!runtime.observe_use(false, false, false, true));
        let clock = RepeatClock {
            now_millis: 1_000,
            sneaking: false,
            speed: 0.0,
            survival: true,
        };
        assert_eq!(runtime.due(false, 1, clock), None);
    }

    #[test]
    fn position_corrections_preserve_active_holds_and_refused_stops() {
        for refused in [false, true] {
            let mut runtime = BlockUseRuntime::default();
            runtime.synchronize((7, 0));
            runtime.intention.record(
                false,
                [0, 63, 1],
                LocalUse::Place,
                true,
                false,
                [0.5, 64.0, 0.5],
            );
            if refused {
                assert_eq!(runtime.stop_packets(42, true).len(), 1);
                assert!(!runtime.admit_stop(false));
            }
            runtime.synchronize((7, 1));
            assert_eq!(runtime.last_success_destination(), Some([0, 63, 1]));
            assert_eq!(runtime.stopping(), refused);
            assert_eq!(runtime.stop_packets(42, false).len(), 1);
            assert!(runtime.admit_stop(true));
            let clock = RepeatClock {
                now_millis: 1_000,
                sneaking: false,
                speed: 0.0,
                survival: true,
            };
            assert_eq!(
                runtime.due(false, 1, clock),
                refused.then_some((ItemUseTrigger::PlayerInput, 1_000)),
            );
        }
    }

    #[test]
    fn selection_changes_preserve_a_press_latched_between_physics_ticks() {
        let mut runtime = BlockUseRuntime::default();
        let stack = protocol::NetworkItemStack::empty();
        let item =
            protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
        let mut selection = crate::mining::FrozenMiningSelection { slot: 0, item };
        assert!(!runtime.selection_changed(&selection));
        runtime.intention.record(
            false,
            [0, 63, 1],
            LocalUse::Place,
            true,
            false,
            [0.5, 64.0, 0.5],
        );
        assert!(runtime.observe_use(true, true, false, true));
        selection.slot = 1;
        assert!(runtime.selection_changed(&selection));
        assert!(!runtime.stopping());
        let clock = RepeatClock {
            now_millis: 1_000,
            sneaking: false,
            speed: 0.0,
            survival: true,
        };
        assert!(runtime.observe_use(false, false, false, true));
        assert_eq!(
            runtime.due(false, 1, clock),
            Some((ItemUseTrigger::PlayerInput, 1_000))
        );
    }

    #[test]
    fn release_before_a_success_has_no_stop_action() {
        let mut runtime = BlockUseRuntime::default();
        assert!(runtime.stop_packets(42, false).is_empty());
        assert!(runtime.admit_stop(true));
        assert!(!runtime.observe_use(false, false, false, true));
    }
}
