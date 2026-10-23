//! Desktop transitions and their resolved action order.

use crate::{Action, ModifierChord, PhysicalControl};

/// One desktop event in the window backend's arrival order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DesktopEvent {
    Button {
        control: PhysicalControl,
        pressed: bool,
        modifiers: ModifierChord,
    },
    Scroll {
        next: bool,
    },
}

/// A backend gap invalidates a batch rather than inventing a partial gesture.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopEvents {
    events: Vec<DesktopEvent>,
    overflowed: bool,
    initial_modifiers: ModifierChord,
}

impl DesktopEvents {
    /// Records modifiers before the first transition, including modifiers held across frames.
    pub fn set_initial_modifiers(&mut self, modifiers: ModifierChord) {
        self.initial_modifiers = modifiers;
    }

    /// Returns the physical modifier state before replaying this batch.
    pub(crate) const fn initial_modifiers(&self) -> ModifierChord {
        self.initial_modifiers
    }
    /// Retains each transition in the complete backend batch.
    pub fn push(&mut self, event: DesktopEvent) {
        self.events.push(event);
    }

    /// Marks a backend gap whose missing order cannot be reconstructed.
    pub fn invalidate(&mut self) {
        self.overflowed = true;
    }

    /// Borrows the records in source order.
    pub fn events(&self) -> &[DesktopEvent] {
        &self.events
    }

    /// Whether part of the physical sequence is missing.
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }
}

/// Held semantic state at an event, separate from the final frame's projections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionButtons([bool; Action::COUNT]);

impl Default for ActionButtons {
    /// Starts with every semantic action released.
    fn default() -> Self {
        Self([false; Action::COUNT])
    }
}

impl ActionButtons {
    /// Retains the binding resolver's state at this point in the input stream.
    pub const fn new(held: [bool; Action::COUNT]) -> Self {
        Self(held)
    }

    /// Reports the event-time state of one action.
    pub const fn held(self, action: Action) -> bool {
        self.0[action as usize]
    }
}

/// One resolved transition; repeated presses remain distinct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionEvent {
    pub action: Action,
    pub pressed: bool,
    pub buttons: ActionButtons,
}

/// Events finalized under the frame's bindings, context and authority.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrderedActions {
    pub(crate) initial_buttons: ActionButtons,
    pub(crate) events: Vec<ActionEvent>,
    overflowed: bool,
    neutral: bool,
}

impl OrderedActions {
    /// Records the resolved held state before the first desktop transition.
    pub(crate) fn set_initial_buttons(&mut self, buttons: ActionButtons) {
        self.initial_buttons = buttons;
    }

    /// Whether an event after the consumed prefix releases `action`.
    pub fn releases_after(&self, consumed: usize, action: Action) -> bool {
        self.events.get(consumed..).is_some_and(|events| {
            events
                .iter()
                .any(|event| event.action == action && !event.pressed)
        })
    }

    /// Returns held state after the consumed prefix, clamping past the batch end.
    pub fn buttons_at(&self, consumed: usize) -> ActionButtons {
        self.events[..consumed.min(self.events.len())]
            .last()
            .map_or(self.initial_buttons, |event| event.buttons)
    }

    /// Records physical neutrality independently of filtered Boolean action projections.
    pub(crate) fn set_neutral(&mut self, neutral: bool) {
        self.neutral = neutral;
    }

    /// Whether the complete physical sample contains no held desktop buttons.
    pub const fn neutral(&self) -> bool {
        self.neutral
    }
    /// Discards an incomplete batch while preserving an explicit overflow marker.
    pub(crate) fn invalidate(&mut self) {
        self.events.clear();
        self.overflowed = true;
    }

    /// Retains each action, including repeated transitions and binding fan-out.
    pub(crate) fn push(&mut self, event: ActionEvent) {
        self.events.push(event);
    }

    /// Borrows actions in source order and stable action order within a binding.
    pub fn events(&self) -> &[ActionEvent] {
        &self.events
    }

    /// Whether the batch must be quarantined instead of admitted.
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }
}
