//! Retained observations from the semantic input router.
use crate::{Action, ActionPhase, ActionSnapshot};

/// The last action frame accepted by the semantic router.
#[derive(Debug, Default, Clone)]
pub struct FinalizedInput(Option<ActionSnapshot>);

impl FinalizedInput {
    /// Retains a finalized router result for consumers.
    pub fn from_finalized(snapshot: ActionSnapshot) -> Self {
        Self(Some(snapshot))
    }

    /// Borrows the finalized action frame when one is available.
    #[must_use]
    pub fn snapshot(&self) -> Option<&ActionSnapshot> {
        self.0.as_ref()
    }

    /// Returns movement after input routing and filtering.
    #[must_use]
    pub fn movement(&self) -> [f32; 2] {
        self.0
            .as_ref()
            .map_or([0.0; 2], |snapshot| snapshot.movement)
    }

    /// Returns the original movement axes.
    #[must_use]
    pub fn raw_movement(&self) -> [f32; 2] {
        self.0
            .as_ref()
            .map_or([0.0; 2], |snapshot| snapshot.raw_movement)
    }

    /// Returns the analogue movement axes.
    #[must_use]
    pub fn analogue_movement(&self) -> [f32; 2] {
        self.0
            .as_ref()
            .map_or([0.0; 2], |snapshot| snapshot.analogue_movement)
    }

    /// Returns the finalized look delta.
    #[must_use]
    pub fn look_delta(&self) -> [f32; 2] {
        self.0
            .as_ref()
            .map_or([0.0; 2], |snapshot| snapshot.look_delta)
    }

    /// Returns the finalized phase of one action.
    #[must_use]
    pub fn phase(&self, action: Action) -> ActionPhase {
        self.0.as_ref().map_or(ActionPhase::default(), |snapshot| {
            snapshot.phases[action as usize]
        })
    }

    /// Adds one attack edge to an existing held input without changing its authority.
    pub fn request_mod_attack_press(&mut self) -> bool {
        let Some(snapshot) = self.0.as_mut() else {
            return false;
        };
        let attack = &mut snapshot.phases[Action::Attack as usize];
        if !attack.held || attack.pressed {
            return false;
        }
        attack.pressed = true;
        if let Some(ordered) = snapshot.ordered.as_mut() {
            ordered.push(crate::ActionEvent {
                action: Action::Attack,
                pressed: true,
                buttons: crate::ActionButtons::new(snapshot.phases.map(|phase| phase.held)),
            });
        }
        true
    }

    /// Publishes the router result for this frame.
    pub fn replace(&mut self, snapshot: ActionSnapshot) {
        self.0 = Some(snapshot);
    }

    /// Retires a frame that could not be finalized.
    pub fn clear(&mut self) {
        self.0 = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InputMode;

    /// Builds a frame with independently chosen movement observations.
    fn snapshot_with_carriers(
        movement: [f32; 2],
        raw_movement: [f32; 2],
        analogue_movement: [f32; 2],
    ) -> FinalizedInput {
        FinalizedInput(Some(ActionSnapshot {
            ordered: None,
            frame_sequence: crate::identity::InputSequence::new(1),
            authority_generation: crate::identity::InputAuthorityGeneration::INITIAL,
            movement,
            raw_movement,
            analogue_movement,
            movement_buttons: Default::default(),
            look_delta: [0.0; 2],
            input_mode: InputMode::GamePad,
            phases: [ActionPhase::default(); Action::COUNT],
            release_reasons: [None; Action::COUNT],
            authority_released: false,
        }))
    }

    #[test]
    fn absent_snapshots_report_zero_move_carriers() {
        let snapshot = FinalizedInput::default();
        assert_eq!(snapshot.movement(), [0.0; 2]);
        assert_eq!(snapshot.raw_movement(), [0.0; 2]);
        assert_eq!(snapshot.analogue_movement(), [0.0; 2]);
    }

    #[test]
    fn present_snapshots_expose_each_move_carrier_independently() {
        let snapshot = snapshot_with_carriers([0.6, 0.8], [1.0, 0.8], [0.6, 0.8]);
        assert_eq!(snapshot.movement(), [0.6, 0.8]);
        assert_eq!(snapshot.raw_movement(), [1.0, 0.8]);
        assert_eq!(snapshot.analogue_movement(), [0.6, 0.8]);
    }

    #[test]
    fn personal_attack_pulse_requires_current_held_input() {
        let mut missing = FinalizedInput::default();
        assert!(!missing.request_mod_attack_press());
        assert!(missing.snapshot().is_none());

        let mut released = snapshot_with_carriers([0.0; 2], [0.0; 2], [0.0; 2]);
        released.0.as_mut().unwrap().phases[Action::Attack as usize].released = true;
        let before = released.snapshot().unwrap().clone();
        assert!(!released.request_mod_attack_press());
        assert_eq!(released.snapshot(), Some(&before));
    }

    #[test]
    fn personal_attack_pulse_only_adds_one_attack_edge() {
        let mut input = snapshot_with_carriers([0.6, 0.8], [1.0, 0.8], [0.6, 0.8]);
        let snapshot = input.0.as_mut().unwrap();
        snapshot.phases[Action::Attack as usize].held = true;
        snapshot.phases[Action::Jump as usize].pressed = true;
        snapshot.look_delta = [1.0, -2.0];
        let mut expected = snapshot.clone();
        expected.phases[Action::Attack as usize].pressed = true;

        assert!(input.request_mod_attack_press());
        assert_eq!(input.snapshot(), Some(&expected));
        assert!(!input.request_mod_attack_press());
        assert_eq!(input.snapshot(), Some(&expected));
    }
}
