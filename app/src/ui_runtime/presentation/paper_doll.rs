//! HUD visibility follows HudPlayerRenderer::update (current RVA 09c78c70).

/// Observed state, independent of the user's visibility settings.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct State {
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
pub(super) struct PaperDoll {
    last: Option<u64>,
    remaining: u64,
    armor: Option<[i32; 4]>,
}

impl PaperDoll {
    /// Advances the timer even while settings hide the control; a new session resets it.
    pub(super) fn update(&mut self, now: u64, state: Option<State>) -> bool {
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

/// Captures predicted local movement and authoritative armor without server echo latency.
pub(super) fn observe(
    stream: &client_world::WorldStream,
    runtime: &crate::ui_runtime::UiRuntime,
    player_runtime: &crate::player_runtime::PlayerRuntime,
    physics: &crate::movement::LocalPhysicsController,
) -> Option<State> {
    use crate::ui_runtime::inventory_ledger::InventoryTarget;
    let actor = stream.actor(stream.local_player_runtime_id())?;
    let flag = |bit: u32| {
        let key = if bit < 64 { 0 } else { 92 };
        match actor.metadata.get(&key) {
            Some(
                protocol::ActorMetadataValue::Flags(bits)
                | protocol::ActorMetadataValue::FlagsExtended(bits),
            ) => bits & (1 << (bit % 64)) != 0,
            _ => false,
        }
    };
    let (sneaking, sprinting) = physics.latest_sneak_sprint().unwrap_or((flag(1), flag(3)));
    Some(State {
        sneaking,
        sprinting,
        in_water: physics.in_water(),
        swimming: physics.mode() == sim::MovementMode::Swimming || flag(57),
        crawling: flag(114),
        flying: physics.mode() == sim::MovementMode::Flying,
        gliding: physics.mode() == sim::MovementMode::Gliding || flag(32),
        emoting: flag(92),
        armor: std::array::from_fn(|slot| {
            runtime
                .inventory_ledger(player_runtime)
                .target_stack(InventoryTarget::Armor(slot as u8))
                .map_or(0, |stack| stack.network_id)
        }),
    })
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
