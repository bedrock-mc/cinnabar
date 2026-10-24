//! Subtractive press limiting after input authority and bindings have been resolved.

use crate::{Action, ActionButtons, ActionPhase, ActionSnapshot};

const ACTIONS: [Action; 2] = [Action::Attack, Action::Use];

/// Keeps accepted held gestures intact and discards excess gestures until release.
#[derive(Default)]
pub struct ClickLimiter {
    channels: [Channel; 2],
    generation: Option<crate::identity::InputAuthorityGeneration>,
}

#[derive(Default)]
struct Channel {
    rate: Option<u32>,
    remaining: f64,
    blocked: bool,
}

impl Channel {
    /// Advances one cap without accumulating credits for a later burst.
    fn advance(&mut self, rate: Option<u32>, seconds: f64) {
        if rate != self.rate {
            self.remaining = 0.;
            self.rate = rate;
        }
        if rate.is_none() {
            self.blocked = false;
        }
        self.remaining = (self.remaining - seconds).max(0.);
    }

    /// Admits only a physical press beyond the previous accepted press's interval.
    fn edge(&mut self, pressed: bool) -> bool {
        let Some(rate) = self.rate else {
            return true;
        };
        if !pressed {
            return !std::mem::take(&mut self.blocked);
        }
        self.blocked = self.remaining > 1e-9;
        if !self.blocked {
            self.remaining = 1. / f64::from(rate);
        }
        !self.blocked
    }
}

impl ClickLimiter {
    /// Drops cooldowns and interrupted gestures at gameplay/session authority boundaries.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Filters ordered transitions and aggregate phases without generating an action.
    pub(crate) fn apply(
        &mut self,
        input: &mut ActionSnapshot,
        rates: [Option<u32>; 2],
        seconds: f64,
    ) {
        if rates == [None; 2] {
            self.reset();
            return;
        }
        if input.authority_released {
            self.reset();
            return;
        }
        if self.generation != Some(input.authority_generation) {
            self.reset();
            self.generation = Some(input.authority_generation);
        }
        let seconds = if seconds.is_finite() {
            seconds.max(0.)
        } else {
            0.
        };
        for (channel, rate) in self.channels.iter_mut().zip(rates) {
            channel.advance(rate.filter(|rate| *rate > 0), seconds);
        }
        if let Some(ordered) = &mut input.ordered {
            ordered.initial_buttons = self.buttons(ordered.initial_buttons);
            let mut edges = [ActionPhase::default(); 2];
            ordered.events.retain_mut(|event| {
                if let Some(index) = ACTIONS.iter().position(|action| *action == event.action) {
                    if !self.channels[index].edge(event.pressed) {
                        return false;
                    }
                    if event.pressed {
                        edges[index].pressed = true;
                    } else {
                        edges[index].released = true;
                    }
                }
                event.buttons = self.buttons(event.buttons);
                true
            });
            for (index, action) in ACTIONS.into_iter().enumerate() {
                if self.channels[index].rate.is_some() {
                    let phase = &mut input.phases[action as usize];
                    phase.pressed = edges[index].pressed;
                    phase.released = edges[index].released;
                    phase.held &= !self.channels[index].blocked;
                }
            }
        } else {
            for (channel, action) in self.channels.iter_mut().zip(ACTIONS) {
                let phase = &mut input.phases[action as usize];
                if phase.pressed {
                    phase.pressed = channel.edge(true);
                }
                let blocked = channel.blocked;
                if phase.released {
                    phase.released = channel.edge(false);
                }
                phase.held &= !blocked;
            }
        }
    }

    /// Removes suppressed gesture ownership from every event-time button snapshot.
    fn buttons(&self, buttons: ActionButtons) -> ActionButtons {
        ActionButtons::new(std::array::from_fn(|index| {
            let action = Action::ALL[index];
            buttons.held(action)
                && ACTIONS
                    .iter()
                    .position(|a| *a == action)
                    .is_none_or(|i| !self.channels[i].blocked)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a raw input frame without a window, device, or clock.
    fn frame(attack: ActionPhase, use_: ActionPhase) -> ActionSnapshot {
        let mut phases = [ActionPhase::default(); Action::COUNT];
        phases[Action::Attack as usize] = attack;
        phases[Action::Use as usize] = use_;
        ActionSnapshot {
            ordered: None,
            frame_sequence: crate::identity::InputSequence::new(1),
            authority_generation: crate::identity::InputAuthorityGeneration::INITIAL,
            movement: [0.; 2],
            raw_movement: [0.; 2],
            analogue_movement: [0.; 2],
            movement_buttons: Default::default(),
            look_delta: [0.; 2],
            input_mode: crate::InputMode::KeyboardMouse,
            phases,
            release_reasons: [None; Action::COUNT],
            authority_released: false,
        }
    }

    /// Represents a complete physical click within one frame.
    fn click() -> ActionPhase {
        ActionPhase {
            pressed: true,
            released: true,
            held: false,
        }
    }

    /// No stored credit may burst; the channels have independent cooldowns.
    #[test]
    fn caps_channels_without_delaying_or_synthesizing_clicks() {
        let mut limiter = ClickLimiter::default();
        let mut first = frame(click(), click());
        limiter.apply(&mut first, [Some(10), Some(20)], 0.);
        assert!(first.phases[Action::Attack as usize].pressed);
        let mut next = frame(click(), click());
        limiter.apply(&mut next, [Some(10), Some(20)], 0.05);
        assert!(!next.phases[Action::Attack as usize].pressed);
        assert!(next.phases[Action::Use as usize].pressed);
        let mut next = frame(click(), click());
        limiter.apply(&mut next, [Some(10), Some(20)], 0.05);
        assert!(next.phases[Action::Attack as usize].pressed);
        let mut idle = frame(Default::default(), Default::default());
        limiter.apply(&mut idle, [Some(10), Some(20)], 10.);
        assert_eq!(idle.phases[Action::Attack as usize], ActionPhase::default());
    }

    /// A rejected press cannot become an admitted hold when its cooldown expires.
    #[test]
    fn preserves_accepted_holds_and_rejects_entire_excess_gestures() {
        let mut limiter = ClickLimiter::default();
        let held = ActionPhase {
            held: true,
            ..Default::default()
        };
        let mut first = frame(
            ActionPhase {
                pressed: true,
                ..held
            },
            Default::default(),
        );
        limiter.apply(&mut first, [Some(5), None], 0.);
        let mut next = frame(held, Default::default());
        limiter.apply(&mut next, [Some(5), None], 0.01);
        assert!(next.phases[Action::Attack as usize].held);
        let mut released = frame(
            ActionPhase {
                released: true,
                ..Default::default()
            },
            Default::default(),
        );
        limiter.apply(&mut released, [Some(5), None], 0.01);
        assert!(released.phases[Action::Attack as usize].released);
        let mut blocked = frame(
            ActionPhase {
                pressed: true,
                ..held
            },
            click(),
        );
        limiter.apply(&mut blocked, [Some(5), None], 0.01);
        assert_eq!(
            blocked.phases[Action::Attack as usize],
            ActionPhase::default()
        );
        assert_eq!(blocked.phases[Action::Use as usize], click());
        let mut waiting = frame(held, Default::default());
        limiter.apply(&mut waiting, [Some(5), None], 1.);
        assert!(!waiting.phases[Action::Attack as usize].held);
        limiter.apply(&mut waiting, [None, None], 0.);
        let mut normal = frame(click(), Default::default());
        limiter.apply(&mut normal, [None, None], 0.);
        assert_eq!(normal.phases[Action::Attack as usize], click());
    }

    /// Every event-time projection must agree with filtered ordered edges.
    #[test]
    fn filters_same_frame_bursts_and_masks_unrelated_event_snapshots() {
        let mut limiter = ClickLimiter::default();
        let mut input = frame(
            ActionPhase {
                pressed: true,
                released: true,
                held: true,
            },
            Default::default(),
        );
        let mut order = crate::OrderedActions::default();
        let buttons = |held| {
            let mut b = [false; Action::COUNT];
            b[Action::Attack as usize] = held;
            ActionButtons::new(b)
        };
        for (action, pressed, held) in [
            (Action::Attack, true, true),
            (Action::Attack, false, false),
            (Action::Attack, true, true),
            (Action::Jump, true, true),
        ] {
            order.push(crate::ActionEvent {
                action,
                pressed,
                buttons: buttons(held),
            });
        }
        input.ordered = Some(order);
        limiter.apply(&mut input, [Some(10), None], 0.);
        let ordered = input.ordered.as_ref().unwrap();
        assert_eq!(ordered.events().len(), 3);
        assert_eq!(ordered.events()[2].action, Action::Jump);
        assert!(!ordered.events()[2].buttons.held(Action::Attack));
        assert_eq!(input.phases[Action::Attack as usize], click());
        let mut next = frame(
            ActionPhase {
                released: true,
                ..Default::default()
            },
            Default::default(),
        );
        limiter.apply(&mut next, [Some(10), None], 0.01);
        assert!(!next.phases[Action::Attack as usize].released);
    }

    /// Non-finite deltas cannot bypass a cap, and authority loss clears the old session.
    #[test]
    fn invalid_time_and_authority_boundaries_do_not_replay_presses() {
        let mut limiter = ClickLimiter::default();
        let mut input = frame(click(), Default::default());
        limiter.apply(&mut input, [Some(1), None], 0.);
        for dt in [f64::NAN, f64::INFINITY, -1.] {
            let mut next = frame(click(), Default::default());
            limiter.apply(&mut next, [Some(1), None], dt);
            assert!(!next.phases[Action::Attack as usize].pressed);
        }
        limiter.reset();
        let mut next = frame(click(), Default::default());
        limiter.apply(&mut next, [Some(1), None], 0.);
        assert!(next.phases[Action::Attack as usize].pressed);
        let mut changed = frame(click(), Default::default());
        changed.authority_generation =
            crate::identity::InputAuthorityGeneration::new(core::num::NonZeroU64::new(2).unwrap());
        limiter.apply(&mut changed, [Some(1), None], 0.);
        assert!(changed.phases[Action::Attack as usize].pressed);
        let mut boundary = frame(
            ActionPhase {
                released: true,
                ..Default::default()
            },
            Default::default(),
        );
        boundary.authority_released = true;
        let before = boundary.clone();
        limiter.apply(&mut boundary, [Some(1), None], 0.);
        assert_eq!(boundary, before);
    }
}
