//! Policy-free application of a guest's decisions to resolved button transitions.

use crate::{Action, ActionButtons, ActionEvent, ActionPhase, ActionSnapshot};

/// Button actions admitted by the first action-filter contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterAction {
    Attack,
    ItemUse,
}
impl FilterAction {
    pub const ALL: [Self; 2] = [Self::Attack, Self::ItemUse];
    /// The semantic action whose bindings produced this transition.
    pub const fn action(self) -> Action {
        match self {
            Self::Attack => Action::Attack,
            Self::ItemUse => Action::Use,
        }
    }
    /// Stable channel index within this contract.
    pub const fn index(self) -> usize {
        self as usize
    }
    /// Selects only actions admitted by this version of the contract.
    fn from_action(action: Action) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.action() == action)
    }
}
/// A physical edge, or an independently authorized press on a physical hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionKind {
    Press,
    Release,
    Repeat,
}
/// One resolved, source-backed action transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionTransition {
    pub action: FilterAction,
    pub kind: TransitionKind,
}
/// Immutable observations for a synchronous filter invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionFilterFrame {
    pub epoch: u64,
    pub time_us: u64,
    pub physical: [bool; 2],
    pub logical: [bool; 2],
    pub transitions: Vec<ActionTransition>,
}
/// One decision for each observed transition, plus completion of previously retained releases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionVerdict {
    pub accepted: Vec<bool>,
    pub release: [bool; 2],
}
impl ActionVerdict {
    /// Passes observed transitions and finishes outstanding releases without adding presses.
    pub fn passthrough(frame: &ActionFilterFrame) -> Self {
        Self {
            accepted: vec![true; frame.transitions.len()],
            release: [true; 2],
        }
    }
}
/// Native ownership and projection state; no feature settings, timers or rate decisions.
#[derive(Default)]
pub struct ActionFilterState {
    logical: Option<[bool; 2]>,
    physical: Option<[bool; 2]>,
    owner: Option<u64>,
    generation: Option<crate::identity::InputAuthorityGeneration>,
    epoch: u64,
    interrupted: bool,
}
impl ActionFilterState {
    /// Invalidates continuity; the next sample completes retained releases before its edges.
    pub fn reset(&mut self) {
        self.interrupted = true;
    }

    /// Calls one owner only when edges or retained holds need decisions, then atomically
    /// projects its verdict. Missing/invalid verdicts restore source-backed input. Suppressed
    /// gestures remain suppressed until their release; failure never manufactures a press.
    /// Returns whether the separately authorized repeat was admitted.
    pub fn filter(
        &mut self,
        input: &mut ActionSnapshot,
        owner: Option<u64>,
        time_us: u64,
        repeat: bool,
        mut decide: impl FnMut(&ActionFilterFrame) -> Option<ActionVerdict>,
    ) -> bool {
        let interrupted = self.interrupted
            || self.owner != owner
            || self.generation != Some(input.authority_generation)
            || input.authority_released
            || input.ordered.as_ref().is_some_and(|o| o.overflowed());
        let initial = input.ordered.as_ref().map_or_else(
            || {
                ActionButtons::new(std::array::from_fn(|i| {
                    let phase = input.phases[i];
                    FilterAction::from_action(Action::ALL[i])
                        .and_then(|a| {
                            (!interrupted)
                                .then_some(self.physical)
                                .flatten()
                                .map(|p| p[a.index()])
                        })
                        .unwrap_or(
                            (phase.held || phase.released)
                                && (!phase.pressed || phase.held && phase.released),
                        )
                }))
            },
            |o| o.initial_buttons,
        );
        let invalid =
            input.authority_released || input.ordered.as_ref().is_some_and(|o| o.overflowed());
        let physical = if invalid {
            [false; 2]
        } else {
            FilterAction::ALL.map(|a| initial.held(a.action()))
        };
        let physical_end = FilterAction::ALL.map(|a| input.phases[a.action() as usize].held);
        let logical = self.logical.unwrap_or(physical);
        if interrupted {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.owner = owner;
        self.generation = Some(input.authority_generation);
        self.interrupted = false;
        let mut events = input
            .ordered
            .as_ref()
            .map_or_else(|| aggregate_events(input, initial), |o| o.events.clone());
        if invalid {
            events.retain(|e| FilterAction::from_action(e.action).is_none());
        }
        let mut transitions: Vec<_> = events
            .iter()
            .filter_map(|e| {
                Some(ActionTransition {
                    action: FilterAction::from_action(e.action)?,
                    kind: if e.pressed {
                        TransitionKind::Press
                    } else {
                        TransitionKind::Release
                    },
                })
            })
            .collect();
        // Repeat authority is independent of filtering and never extends beyond physical release.
        let repeat = repeat
            && !invalid
            && input.phases[Action::Attack as usize].held
            && !input.phases[Action::Attack as usize].pressed;
        if repeat {
            transitions.push(ActionTransition {
                action: FilterAction::Attack,
                kind: TransitionKind::Repeat,
            });
        }
        let frame = ActionFilterFrame {
            epoch: self.epoch,
            time_us,
            physical,
            logical: std::array::from_fn(|i| logical[i] && (!interrupted || physical[i])),
            transitions,
        };
        if frame.transitions.is_empty() && logical == physical && !interrupted && !invalid {
            self.logical = Some(logical);
            self.physical = Some(physical_end);
            return false;
        }
        let callable = owner.is_some()
            && !input.authority_released
            && !input.ordered.as_ref().is_some_and(|o| o.overflowed());
        let retained = logical.into_iter().zip(physical).any(|(l, p)| l && !p);
        let mut verdict = if callable && (!frame.transitions.is_empty() || retained) {
            decide(&frame).filter(|v| v.accepted.len() == frame.transitions.len())
        } else {
            Some(ActionVerdict::passthrough(&frame))
        };
        if verdict.is_none() {
            self.epoch = self.epoch.wrapping_add(1);
            self.interrupted = true;
        }
        let failed = verdict.is_none();
        let mut verdict = verdict
            .take()
            .unwrap_or_else(|| ActionVerdict::passthrough(&frame));
        if interrupted || failed {
            verdict.release = [true; 2];
        }
        let mut held = logical;
        let mut edges = [ActionPhase::default(); 2];
        let mut output = Vec::with_capacity(events.len() + 3);
        for a in FilterAction::ALL {
            let i = a.index();
            if verdict.release[i] && held[i] && !physical[i] {
                emit(&mut output, &mut edges, &mut held, a, false, initial);
            }
        }
        let mut physical = physical;
        let mut decision = 0;
        for event in events {
            if let Some(a) = FilterAction::from_action(event.action) {
                let i = a.index();
                let accepted = verdict.accepted[decision];
                decision += 1;
                if accepted {
                    if event.pressed && (!physical[i] || held[i]) {
                        if held[i] && !physical[i] {
                            emit(&mut output, &mut edges, &mut held, a, false, event.buttons);
                        }
                        emit(&mut output, &mut edges, &mut held, a, true, event.buttons);
                    } else if held[i] {
                        emit(&mut output, &mut edges, &mut held, a, false, event.buttons);
                    }
                }
                physical[i] = event.pressed;
            } else {
                output.push(ActionEvent {
                    buttons: project(event.buttons, held),
                    ..event
                });
            }
        }
        let admitted = repeat && verdict.accepted[decision];
        if admitted {
            let buttons = ActionButtons::new(input.phases.map(|p| p.held));
            emit(
                &mut output,
                &mut edges,
                &mut held,
                FilterAction::Attack,
                true,
                buttons,
            );
        }
        if let Some(ordered) = &mut input.ordered {
            ordered.initial_buttons = project(initial, logical);
            ordered.events = output;
        }
        for a in FilterAction::ALL {
            let i = a.index();
            input.phases[a.action() as usize] = ActionPhase {
                held: held[i],
                ..edges[i]
            };
        }
        self.logical = Some(held);
        self.physical = Some(physical_end);
        admitted
    }
}
/// Emits an admitted edge and its consistent logical button snapshot.
fn emit(
    events: &mut Vec<ActionEvent>,
    edges: &mut [ActionPhase; 2],
    held: &mut [bool; 2],
    action: FilterAction,
    pressed: bool,
    raw: ActionButtons,
) {
    let i = action.index();
    held[i] = pressed;
    if pressed {
        edges[i].pressed = true;
    } else {
        edges[i].released = true;
    }
    events.push(ActionEvent {
        action: action.action(),
        pressed,
        buttons: project(raw, *held),
    });
}
/// Replaces only the actions covered by this contract in an event-time projection.
fn project(raw: ActionButtons, held: [bool; 2]) -> ActionButtons {
    ActionButtons::new(std::array::from_fn(|i| {
        FilterAction::from_action(Action::ALL[i])
            .map_or_else(|| raw.held(Action::ALL[i]), |a| held[a.index()])
    }))
}
/// Defines stable ordering for phase-only devices: an existing hold releases first;
/// a new gesture presses first. These samples cannot recover sub-frame device order.
fn aggregate_events(input: &ActionSnapshot, initial: ActionButtons) -> Vec<ActionEvent> {
    let mut events = Vec::new();
    let mut buttons = initial;
    for a in FilterAction::ALL {
        let action = a.action();
        let p = input.phases[action as usize];
        let order = if p.pressed && p.released && p.held {
            [Some(false), Some(true)]
        } else {
            [p.pressed.then_some(true), p.released.then_some(false)]
        };
        for pressed in order.into_iter().flatten() {
            buttons = ActionButtons::new(std::array::from_fn(|i| {
                if i == action as usize {
                    pressed
                } else {
                    buttons.held(Action::ALL[i])
                }
            }));
            events.push(ActionEvent {
                action,
                pressed,
                buttons,
            });
        }
    }
    events
}

#[cfg(test)]
#[path = "action_filter_tests.rs"]
mod tests;
