//! Eager presses and deferred releases keep switch chatter within one held gesture.

use crate::{Action, ActionButtons, ActionEvent, ActionPhase, ActionSnapshot};

const ACTIONS: [Action; 2] = [Action::Attack, Action::Use];

/// Debounces independent attack/use gestures after input authority is resolved.
#[derive(Default)]
pub struct ClickDebouncer {
    channels: [Channel; 2],
    generation: Option<crate::identity::InputAuthorityGeneration>,
}

#[derive(Default)]
struct Channel {
    interval: Option<u32>,
    held: bool,
    release: Option<f64>,
}

impl Channel {
    /// Ages an owned release, flushing it when filtering is disabled or its timer expires.
    fn advance(&mut self, interval: Option<u32>, initial: bool, seconds: f64) -> bool {
        if self.interval.is_none() {
            self.held = initial;
            self.release = None;
        }
        self.interval = interval;
        if let Some(ms) = interval {
            if let Some(remaining) = &mut self.release {
                *remaining = (*remaining - seconds).min(f64::from(ms) / 1_000.);
                if *remaining <= 1e-9 {
                    self.release = None;
                    self.held = false;
                    return true;
                }
            }
            false
        } else {
            let released = self.held && !initial;
            self.held = initial;
            self.release = None;
            released
        }
    }

    /// Accepts a fresh press immediately and joins re-presses before a deferred release.
    fn edge(&mut self, pressed: bool) -> bool {
        let Some(ms) = self.interval else {
            self.held = pressed;
            return true;
        };
        if pressed {
            self.release = None;
            !std::mem::replace(&mut self.held, true)
        } else {
            if self.held {
                self.release = Some(f64::from(ms) / 1_000.);
            }
            false
        }
    }
}

impl ClickDebouncer {
    /// Drops pending releases when gameplay/session authority is interrupted.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Coalesces bounced transitions and publishes each physical release once it settles.
    pub(crate) fn apply(
        &mut self,
        input: &mut ActionSnapshot,
        intervals: [Option<u32>; 2],
        seconds: f64,
    ) {
        let intervals = intervals.map(|interval| interval.filter(|ms| *ms > 0));
        if intervals == [None; 2]
            && self
                .channels
                .iter()
                .all(|channel| channel.interval.is_none())
        {
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
        let initial = input.ordered.as_ref().map_or_else(
            || {
                ActionButtons::new(
                    input
                        .phases
                        .map(|phase| (phase.held || phase.released) && !phase.pressed),
                )
            },
            |ordered| ordered.initial_buttons,
        );
        let managed = std::array::from_fn::<_, 2, _>(|i| {
            self.channels[i].interval.is_some() || intervals[i].is_some()
        });
        let before = std::array::from_fn(|i| {
            if self.channels[i].interval.is_some() {
                self.channels[i].held
            } else {
                initial.held(ACTIONS[i])
            }
        });
        let mut edges = [ActionPhase::default(); 2];
        let released = std::array::from_fn::<_, 2, _>(|i| {
            let released = self.channels[i].advance(
                intervals[i].filter(|ms| *ms > 0),
                initial.held(ACTIONS[i]),
                seconds,
            );
            edges[i].released = released;
            released
        });
        if let Some(ordered) = &mut input.ordered {
            ordered.initial_buttons = project(initial, before, managed);
            ordered.events.retain_mut(|event| {
                if let Some(i) = ACTIONS.iter().position(|action| *action == event.action)
                    && managed[i]
                {
                    if !self.channels[i].edge(event.pressed) {
                        return false;
                    }
                    if event.pressed {
                        edges[i].pressed = true;
                    } else {
                        edges[i].released = true;
                    }
                }
                event.buttons = project(
                    event.buttons,
                    self.channels.each_ref().map(|c| c.held),
                    managed,
                );
                true
            });
            let mut held = before;
            let mut prefix = [None; 2];
            for (i, action) in ACTIONS.into_iter().enumerate() {
                if released[i] {
                    held[i] = false;
                    prefix[i] = Some(ActionEvent {
                        action,
                        pressed: false,
                        buttons: project(initial, held, managed),
                    });
                }
            }
            // At most two releases owned by preceding frames precede the current physical batch.
            for event in prefix.into_iter().rev().flatten() {
                ordered.events.insert(0, event);
            }
        } else {
            for (i, action) in ACTIONS.into_iter().enumerate() {
                if !managed[i] {
                    continue;
                }
                let phase = input.phases[action as usize];
                if phase.pressed {
                    edges[i].pressed = self.channels[i].edge(true);
                }
                if phase.released && !phase.held {
                    edges[i].released |= self.channels[i].edge(false);
                }
            }
        }
        for (i, action) in ACTIONS.into_iter().enumerate() {
            if managed[i] {
                input.phases[action as usize] = ActionPhase {
                    held: self.channels[i].held,
                    ..edges[i]
                };
            }
        }
    }
}

/// Keeps all event-time button projections consistent with the debounced logical hold.
fn project(raw: ActionButtons, held: [bool; 2], managed: [bool; 2]) -> ActionButtons {
    ActionButtons::new(std::array::from_fn(|i| {
        let action = Action::ALL[i];
        ACTIONS
            .iter()
            .position(|a| *a == action)
            .filter(|i| managed[*i])
            .map_or_else(|| raw.held(action), |i| held[i])
    }))
}

#[cfg(test)]
#[path = "click_debounce_tests.rs"]
mod tests;
