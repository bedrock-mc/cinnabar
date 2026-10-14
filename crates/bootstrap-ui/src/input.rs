//! Focus and press capture shared by pointer, keyboard and controller input.

use launcher::menu::settings_options::SettingsOptions;
use winit::{
    event::ElementState,
    keyboard::{Key, NamedKey},
};

/// Where an input came from; a press only activates on a release from the same source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Pointer,
    Keyboard,
    Controller,
}

/// A screen's focusable buttons, in focus order.
pub trait Actions {
    type Action: Copy + Eq;
    /// Enabled buttons in focus order; a reset focuses the first.
    fn actions(&self) -> &[Self::Action];
    /// What Escape and the controller's back button activate.
    fn cancel(&self) -> Self::Action;
}

/// Keyboard and controller focus plus the press being held, for one window's buttons.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Input<A> {
    /// The button keyboard and controller presses act on.
    pub focused: Option<A>,
    /// Whether the focus outline shows; pointer use hides it.
    pub focus_visible: bool,
    capture: Option<(A, Source)>,
}

impl<A> Default for Input<A> {
    fn default() -> Self {
        Self {
            focused: None,
            focus_visible: false,
            capture: None,
        }
    }
}

impl<A: Copy + Eq> Input<A> {
    /// Keeps focus on an available action and drops presses when the screen changes.
    pub fn reset(&mut self, screen: &impl Actions<Action = A>) {
        self.focused = screen.actions().first().copied();
        self.capture = None;
    }

    /// Worker phases with the same actions keep a held Cancel press and navigation focus.
    pub fn update_screen<S: Actions<Action = A>>(&mut self, before: &S, after: &S) {
        if before.actions() != after.actions() {
            self.reset(after);
        }
    }

    /// Moves through available actions with wrapping for both keyboard and D-pad navigation.
    pub fn navigate(&mut self, screen: &impl Actions<Action = A>, backwards: bool) {
        let actions = screen.actions();
        if actions.is_empty() {
            return;
        }
        let current = self
            .focused
            .and_then(|a| actions.iter().position(|b| *b == a));
        let index = match current {
            None => 0,
            Some(i) if backwards => (i + actions.len() - 1) % actions.len(),
            Some(i) => (i + 1) % actions.len(),
        };
        self.focused = Some(actions[index]);
        self.focus_visible = true;
        self.capture = None;
    }

    /// Captures an enabled action until the same input source releases it.
    pub fn press(&mut self, screen: &impl Actions<Action = A>, source: Source, hit: Option<A>) {
        let action = if source == Source::Pointer {
            hit
        } else {
            self.focused
        };
        if let Some(action) = action.filter(|a| screen.actions().contains(a)) {
            self.focused = Some(action);
            self.focus_visible = source != Source::Pointer;
            self.capture = Some((action, source));
        }
    }

    /// Activates only a matching release; dragging away from a pointer press cancels it.
    pub fn release(&mut self, source: Source, hit: Option<A>) -> Option<A> {
        let (action, captured) = self.capture?;
        if captured != source {
            return None;
        }
        self.capture = None;
        (source != Source::Pointer || hit == Some(action)).then_some(action)
    }

    /// The visible depressed face follows the pointer while a pointer press is held.
    pub fn pressed(&self, hovered: Option<A>) -> Option<A> {
        self.capture.and_then(|(action, source)| {
            (source != Source::Pointer || hovered == Some(action)).then_some(action)
        })
    }

    /// Losing native focus cancels a held press without firing an action.
    pub fn blur(&mut self) {
        self.capture = None;
    }
}

/// Controller commands use the same state machine as keyboard commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Navigate(bool),
    Press,
    Release,
    Cancel,
    Blur,
}

/// Converts gamepad transitions without accepting held-button repeat events.
pub fn controller(event: gilrs::EventType, settings: &SettingsOptions) -> Option<Command> {
    match event {
        gilrs::EventType::ButtonPressed(button, _) => controller_button(button, true, settings),
        gilrs::EventType::ButtonReleased(button, _) => controller_button(button, false, settings),
        gilrs::EventType::Disconnected => Some(Command::Blur),
        _ => None,
    }
}

/// Applies saved confirmation settings to the bootstrap controller buttons.
fn controller_button(
    button: gilrs::Button,
    pressed: bool,
    settings: &SettingsOptions,
) -> Option<Command> {
    use gilrs::Button;
    use semantic_input::PhysicalControl;
    let button = match button {
        Button::DPadLeft | Button::DPadUp if pressed => return Some(Command::Navigate(true)),
        Button::DPadRight | Button::DPadDown if pressed => return Some(Command::Navigate(false)),
        Button::South => PhysicalControl::GamepadButton(0),
        Button::East => PhysicalControl::GamepadButton(1),
        _ => return None,
    };
    match (settings.swap_gamepad_control(button), pressed) {
        (PhysicalControl::GamepadButton(0), true) => Some(Command::Press),
        (PhysicalControl::GamepadButton(0), false) => Some(Command::Release),
        (PhysicalControl::GamepadButton(1), true) => Some(Command::Cancel),
        _ => None,
    }
}

impl<A: Copy + Eq> Input<A> {
    /// Returns an action and whether input changed the overlay's interaction state.
    pub fn apply(
        &mut self,
        screen: &impl Actions<Action = A>,
        source: Source,
        command: Command,
        hit: Option<A>,
    ) -> (Option<A>, bool) {
        let before = *self;
        let action = match command {
            Command::Navigate(backwards) => {
                self.navigate(screen, backwards);
                None
            }
            Command::Press => {
                self.press(screen, source, hit);
                None
            }
            Command::Release => self.release(source, hit),
            Command::Cancel => Some(screen.cancel()),
            Command::Blur => {
                self.blur();
                None
            }
        };
        (action, *self != before)
    }

    /// Synthetic focus events and unrelated keys cannot activate actions or dirty the overlay.
    pub fn keyboard(
        &mut self,
        screen: &impl Actions<Action = A>,
        key: &Key,
        state: ElementState,
        shift: bool,
        synthetic: bool,
    ) -> (Option<A>, bool) {
        if synthetic {
            return (None, false);
        }
        let pressed = state == ElementState::Pressed;
        let command = match key {
            Key::Named(NamedKey::Tab) if pressed => Some(Command::Navigate(shift)),
            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowUp) if pressed => {
                Some(Command::Navigate(true))
            }
            Key::Named(NamedKey::ArrowRight | NamedKey::ArrowDown) if pressed => {
                Some(Command::Navigate(false))
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => Some(if pressed {
                Command::Press
            } else {
                Command::Release
            }),
            Key::Named(NamedKey::Escape) if pressed => Some(Command::Cancel),
            _ => None,
        };
        command.map_or((None, false), |command| {
            self.apply(screen, Source::Keyboard, command, None)
        })
    }
}

pub mod pointer;

#[cfg(test)]
mod regression_tests;
