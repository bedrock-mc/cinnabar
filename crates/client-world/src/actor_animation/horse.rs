use super::*;

pub(super) const KEY_FLAGS: u32 = 16;
const FLAG_GRAZING: u32 = 5;
const FLAG_MOUTH_OPEN: u32 = 7;
const TAIL_CHANCE: u64 = 200;
const TAIL_END: u8 = 8;

fn flag(actor: &ActorSnapshot, bit: u32) -> bool {
    matches!(actor.metadata.get(&KEY_FLAGS), Some(ActorMetadataValue::Long(flags))
        if (*flags as u64) & (1_u64 << bit) != 0)
}

pub(super) fn is_grazing(actor: &ActorSnapshot) -> bool {
    flag(actor, FLAG_GRAZING)
}

pub(super) fn mouth_open(actor: &ActorSnapshot) -> bool {
    flag(actor, FLAG_MOUTH_OPEN)
}

/// Horse animation state survives geometry and controller changes within an actor lifetime.
#[derive(Clone, Copy, Debug)]
pub(super) struct AnimationState {
    pub(super) stand_amount: f32,
    tail_ticks: u8,
    random: u64,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self::new(1)
    }
}

impl AnimationState {
    pub(super) fn new(seed: u64) -> Self {
        Self {
            stand_amount: 0.0,
            tail_ticks: 0,
            random: seed.max(1),
        }
    }

    pub(super) fn advance(&mut self, standing: bool) {
        // Independent actor random streams preserve the native uniform tail-start probability.
        let start_tail = loop {
            self.random ^= self.random << 13;
            self.random ^= self.random >> 7;
            self.random ^= self.random << 17;
            let value = self.random.wrapping_mul(0x2545_f491_4f6c_dd1d);
            if value >= TAIL_CHANCE.wrapping_neg() % TAIL_CHANCE {
                break value.is_multiple_of(TAIL_CHANCE);
            }
        };
        self.advance_observed(standing, start_tail);
    }

    fn advance_observed(&mut self, standing: bool, start_tail: bool) {
        let amount = self.stand_amount;
        self.stand_amount = if standing {
            (amount + (1.0 - amount) * 0.4 + 0.05).min(1.0)
        } else {
            (amount + (amount * amount * amount * 0.8 - amount) * 0.6 - 0.05).max(0.0)
        };
        if start_tail {
            self.tail_ticks = 1;
        }
        if self.tail_ticks > 0 {
            self.tail_ticks += 1;
            if self.tail_ticks > TAIL_END {
                self.tail_ticks = 0;
            }
        }
    }

    pub(super) fn shake_tail(self) -> bool {
        self.tail_ticks > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_actor_seeds_do_not_synchronize_their_tails() {
        let mut first = AnimationState::new(2);
        let mut second = AnimationState::new(3);
        assert!((0..8192).any(|_| {
            first.advance(false);
            second.advance(false);
            first.shake_tail() != second.shake_tail()
        }));
    }

    #[test]
    fn rearing_rises_smoothly_and_uses_the_cubic_release() {
        let mut state = AnimationState::default();
        state.advance_observed(true, false);
        assert!((state.stand_amount - 0.45).abs() < 1e-6);
        for _ in 0..10 {
            state.advance_observed(true, false);
        }
        assert_eq!(state.stand_amount, 1.0);
        state.advance_observed(false, false);
        assert!((state.stand_amount - 0.83).abs() < 1e-6);
        for _ in 0..20 {
            state.advance_observed(false, false);
        }
        assert_eq!(state.stand_amount, 0.0);
    }

    #[test]
    fn tail_start_is_visible_for_seven_ticks_and_can_restart() {
        let mut state = AnimationState::default();
        state.advance_observed(false, true);
        for _ in 0..6 {
            assert!(state.shake_tail());
            state.advance_observed(false, false);
        }
        assert!(state.shake_tail());
        state.advance_observed(false, false);
        assert!(!state.shake_tail());
        state.advance_observed(false, true);
        state.advance_observed(false, true);
        for _ in 0..6 {
            state.advance_observed(false, false);
        }
        assert!(state.shake_tail());
        state.advance_observed(false, false);
        assert!(!state.shake_tail());
    }

    #[test]
    fn grazing_and_mouth_read_the_horse_word_without_float_conversion() {
        let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
        actor.metadata.insert(
            KEY_FLAGS,
            ActorMetadataValue::Long((1 << 60) | (1 << FLAG_GRAZING)),
        );
        assert!(is_grazing(&actor));
        assert!(!mouth_open(&actor));
        actor
            .metadata
            .insert(KEY_FLAGS, ActorMetadataValue::Long(1 << FLAG_MOUTH_OPEN));
        assert!(mouth_open(&actor));
        assert!(!is_grazing(&actor));
        actor
            .metadata
            .insert(KEY_FLAGS, ActorMetadataValue::Float(f32::NAN));
        assert!(!mouth_open(&actor));
        assert!(!is_grazing(&actor));
    }
}
