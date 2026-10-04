//! HUD visibility follows HudPlayerRenderer::update.

/// Observed state, independent of the user's visibility settings.
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    pub sneaking: bool,
    pub sprinting: bool,
    pub in_water: bool,
    pub swimming: bool,
    pub crawling: bool,
    pub flying: bool,
    pub gliding: bool,
    pub emoting: bool,
    pub armor: [i32; 4],
}

/// Native hold timer. The renderer disappears at expiry without fading.
#[derive(Default)]
pub struct PaperDoll {
    last: Option<u64>,
    remaining: u64,
    armor: Option<[i32; 4]>,
}

impl PaperDoll {
    /// Advances the timer even while settings hide the control; a new session resets it.
    pub fn update(&mut self, now: u64, state: Option<State>) -> bool {
        let Some(state) = state else {
            *self = Self::default();
            return false;
        };
        let elapsed = self
            .last
            .replace(now)
            .map_or(0, |last| now.saturating_sub(last));
        self.remaining = if state.sneaking
            || state.swimming
            || state.crawling
            || (state.sprinting && !state.in_water)
        {
            1_000
        } else if state.flying || state.gliding {
            350
        } else if self.armor != Some(state.armor) {
            self.armor = Some(state.armor);
            3_000
        } else if state.emoting {
            350
        } else {
            self.remaining.saturating_sub(elapsed)
        };
        self.remaining > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Starts after the initial armor observation has expired.
    fn idle() -> PaperDoll {
        let mut doll = PaperDoll::default();
        assert!(doll.update(0, Some(State::default())));
        assert!(!doll.update(3_000, Some(State::default())));
        doll
    }

    #[test]
    fn native_trigger_holds_and_expiry() {
        for state in [
            State {
                sneaking: true,
                ..State::default()
            },
            State {
                sprinting: true,
                ..State::default()
            },
            State {
                swimming: true,
                in_water: true,
                ..State::default()
            },
            State {
                crawling: true,
                ..State::default()
            },
        ] {
            let mut doll = idle();
            assert!(doll.update(4_000, Some(state)));
            assert!(doll.update(4_999, Some(State::default())));
            assert!(!doll.update(5_000, Some(State::default())));
        }
        for state in [
            State {
                flying: true,
                ..State::default()
            },
            State {
                gliding: true,
                ..State::default()
            },
            State {
                emoting: true,
                ..State::default()
            },
        ] {
            let mut doll = idle();
            assert!(doll.update(4_000, Some(state)));
            assert!(doll.update(4_349, Some(State::default())));
            assert!(!doll.update(4_350, Some(State::default())));
        }
    }

    #[test]
    fn wet_sprint_is_not_a_trigger_and_armor_waits_for_movement_to_end() {
        let mut doll = idle();
        assert!(!doll.update(
            4_000,
            Some(State {
                sprinting: true,
                in_water: true,
                ..State::default()
            })
        ));
        let mut state = State {
            armor: [1, 0, 0, 0],
            sneaking: true,
            ..State::default()
        };
        assert!(doll.update(5_000, Some(state)));
        state.sneaking = false;
        assert!(doll.update(6_000, Some(state)));
        assert!(doll.update(8_999, Some(state)));
        assert!(!doll.update(9_000, Some(state)));
        assert!(!doll.update(9_001, None));
        assert!(doll.update(0, Some(state)));
    }
}
