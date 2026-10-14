//! Emote duration on the completed local simulation clock.

use super::MovementTicker;
use protocol::PlayerInputFlags;

impl MovementTicker {
    /// Starts the duration announced by an admitted Emote packet; a later start replaces it.
    pub fn start_emote(&mut self, length_ticks: u32) {
        self.emote_ticks = length_ticks;
    }

    /// Ends the current emote without rewriting already captured movement inputs.
    pub fn stop_emote(&mut self) {
        self.emote_ticks = 0;
    }

    /// Normal ticking consumes duration before the input packet reports the remaining status.
    pub(super) fn advance_emote_input(&mut self) -> PlayerInputFlags {
        self.emote_ticks = self.emote_ticks.saturating_sub(1);
        PlayerInputFlags::NONE.with_mask(PlayerInputFlags::EMOTING, self.emote_ticks != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movement::MovementSource;
    use crate::test_support::survival_mining::completed;

    /// Supplies an active movement queue before the first completed emote tick.
    fn ticker() -> MovementTicker {
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, completed(101).position);
        ticker.set_source(MovementSource::Physics);
        ticker
    }

    /// Reads the encoded flag from one completed movement sample.
    fn next_emoting(ticker: &mut MovementTicker, tick: u64) -> bool {
        ticker.enqueue_completed_physics(completed(tick)).unwrap();
        let snapshot = ticker.pop_pending().unwrap().snapshot;
        let packet = protocol::player_auth_input(snapshot).unwrap();
        let packets = protocol::decode_batch(protocol::encode(&packet).unwrap()).unwrap();
        protocol::player_auth_input_trace_sample(&packets[0])
            .unwrap()
            .flag_names
            .contains(&"Emoting")
    }

    #[test]
    fn emote_duration_expires_before_the_last_ticks_input() {
        for length in [0, 1, 4] {
            let mut ticker = ticker();
            ticker.start_emote(length);
            for elapsed in 1..=5 {
                assert_eq!(
                    next_emoting(&mut ticker, 100 + elapsed),
                    elapsed < u64::from(length)
                );
            }
        }
    }

    #[test]
    fn cancellation_keeps_the_old_input_and_clears_the_next() {
        let mut ticker = ticker();
        ticker.start_emote(4);
        ticker.enqueue_completed_physics(completed(101)).unwrap();
        ticker.stop_emote();
        let captured = ticker.pop_pending().unwrap();
        assert_ne!(
            captured.snapshot.flags.bits() & PlayerInputFlags::EMOTING.bits(),
            0
        );
        ticker.retry_front(captured.clone()).unwrap();
        assert_eq!(ticker.pop_pending().unwrap().snapshot, captured.snapshot);
        assert!(!next_emoting(&mut ticker, 102));
    }

    #[test]
    fn a_new_session_and_a_replacement_emote_retire_the_previous_duration() {
        let mut ticker = ticker();
        ticker.start_emote(20);
        ticker.start_emote(2);
        assert!(next_emoting(&mut ticker, 101));
        assert!(!next_emoting(&mut ticker, 102));
        ticker.start_emote(20);
        ticker.reset(8, 200, completed(201).position);
        ticker.set_source(MovementSource::Physics);
        assert!(!next_emoting(&mut ticker, 201));
    }
}
