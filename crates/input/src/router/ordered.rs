//! Reuses the binding evaluator for each desktop transition, before projecting frame phases.

use super::*;
use crate::{ActionButtons, ActionEvent, DesktopEvent, KeyboardMouseFrame, OrderedActions};

impl SemanticInputRouter {
    /// Resolves each transition under the same context and quarantine as held input.
    pub(super) fn finalize_desktop_events(
        &mut self,
        frame: &DeviceFrame,
        mode: InputMode,
        sample: &mut Sample,
    ) -> Option<OrderedActions> {
        let events = frame.desktop_events.as_ref()?;
        if mode != InputMode::KeyboardMouse {
            return None;
        }
        let mut output = OrderedActions::default();
        output.set_neutral(
            frame.keyboard_mouse.as_ref().is_some_and(|keyboard| {
                keyboard.keys.is_empty() && keyboard.mouse_buttons.is_empty()
            }),
        );
        if events.overflowed() {
            output.invalidate();
            return Some(output);
        }
        if self.pending_release_barrier
            || self.pending_authority_release
            || self.pending_context != Some(self.context)
            || frame.window_focus_lost
        {
            return Some(output);
        }
        let mut state = frame.clone();
        state.desktop_events = None;
        state.controllers.clear();
        state.touches.clear();
        let keyboard = state.keyboard_mouse.as_mut()?;
        keyboard.key_edges = Default::default();
        keyboard.mouse_edges = Default::default();
        keyboard.mouse_motion = [0.0; 2];
        keyboard.modifiers = events.initial_modifiers();
        for event in events.events().iter().rev() {
            if let DesktopEvent::Button {
                control, pressed, ..
            } = *event
            {
                set_button(keyboard, control, !pressed);
            }
        }
        for action in Action::ALL {
            if self.settings.bindings().iter().any(|binding| {
                binding.action == action && control_matches_mode(binding.chord.control, mode)
            }) {
                sample.pressed[action as usize] = false;
                sample.released[action as usize] = false;
            }
        }
        output.set_initial_buttons(ActionButtons::new(self.sample_desktop(&state, mode).active));
        for event in events.events() {
            let previous = state.clone();
            let keyboard = state
                .keyboard_mouse
                .as_mut()
                .expect("desktop sample exists");
            keyboard.key_edges = Default::default();
            keyboard.mouse_edges = Default::default();
            match *event {
                DesktopEvent::Scroll { next } => {
                    if self.context == InputContext::Gameplay {
                        let current = self.sample_desktop(&state, mode);
                        output.push(ActionEvent {
                            action: if next {
                                Action::HotbarNext
                            } else {
                                Action::HotbarPrevious
                            },
                            pressed: true,
                            buttons: ActionButtons::new(current.active),
                        });
                    }
                    continue;
                }
                DesktopEvent::Button {
                    control,
                    pressed,
                    modifiers,
                } => {
                    set_button(keyboard, control, pressed);
                    keyboard.modifiers = modifiers;
                    let quarantined = match control {
                        PhysicalControl::KeyboardUsage(code) => {
                            QuarantinedControl::KeyboardUsage(code)
                        }
                        PhysicalControl::MouseButton(code) => QuarantinedControl::MouseButton(code),
                        _ => unreachable!("validated desktop button"),
                    };
                    if self.quarantined_controls.contains(&quarantined) {
                        if !pressed {
                            self.quarantined_controls
                                .retain(|entry| *entry != quarantined);
                        }
                        continue;
                    }
                    match control {
                        PhysicalControl::KeyboardUsage(code) => {
                            if pressed {
                                &mut keyboard.key_edges.pressed
                            } else {
                                &mut keyboard.key_edges.released
                            }
                            .push(code);
                        }
                        PhysicalControl::MouseButton(code) => {
                            if pressed {
                                &mut keyboard.mouse_edges.pressed
                            } else {
                                &mut keyboard.mouse_edges.released
                            }
                            .push(code);
                        }
                        _ => unreachable!("validated desktop button"),
                    }
                }
            }
            let filtered = without_quarantined_controls(
                &state,
                &self.quarantined_controls,
                &[],
                &[],
                &self.settings,
            );
            let resolved = self.sample_against(&filtered, mode, &previous);
            let before = self.sample_desktop(&previous, mode);
            for action in Action::ALL {
                let index = action as usize;
                let released =
                    resolved.released[index] || (before.active[index] && !resolved.active[index]);
                for pressed in [true, false] {
                    if if pressed {
                        resolved.pressed[index]
                    } else {
                        released
                    } {
                        output.push(ActionEvent {
                            action,
                            pressed,
                            buttons: ActionButtons::new(resolved.active),
                        });
                        if pressed {
                            sample.pressed[index] = true;
                        } else {
                            sample.released[index] = true;
                        }
                    }
                }
            }
        }
        let final_sample = self.sample_desktop(frame, mode);
        let pressed = sample.pressed;
        let released = sample.released;
        *sample = final_sample;
        sample.pressed = pressed;
        sample.released = released;
        Some(output)
    }

    /// Applies desktop quarantine consistently to held state and event-time samples.
    fn sample_desktop(&self, frame: &DeviceFrame, mode: InputMode) -> Sample {
        let filtered = without_quarantined_controls(
            frame,
            &self.quarantined_controls,
            &[],
            &[],
            &self.settings,
        );
        self.sample(&filtered, mode)
    }
}

/// Changes one validated held button without disturbing other controls.
fn set_button(sample: &mut KeyboardMouseFrame, control: PhysicalControl, pressed: bool) {
    match control {
        PhysicalControl::KeyboardUsage(code) => set_held(&mut sample.keys, code, pressed),
        PhysicalControl::MouseButton(code) => set_held(&mut sample.mouse_buttons, code, pressed),
        _ => unreachable!("validated desktop button"),
    }
}

/// Retains a unique held set while allowing an arbitrary ordered transition stream.
fn set_held<T: Copy + PartialEq>(values: &mut Vec<T>, value: T, pressed: bool) {
    values.retain(|candidate| *candidate != value);
    if pressed {
        values.push(value);
    }
}
