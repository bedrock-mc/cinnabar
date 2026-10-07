//! Focus and press capture shared by pointer, keyboard and controller input.

use super::super::screen::{Action, Screen};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Pointer,
    Keyboard,
    Controller,
}

#[derive(Default)]
pub(super) struct Input {
    pub focused: Option<Action>,
    pub focus_visible: bool,
    capture: Option<(Action, Source)>,
}

impl Input {
    /// Keeps focus on an available action and drops presses when the screen changes.
    pub fn reset(&mut self, screen: &Screen) {
        self.focused = screen.actions().first().copied();
        self.capture = None;
    }

    /// Worker phases with the same actions keep a held Cancel press and navigation focus.
    pub fn update_screen(&mut self, before: &Screen, after: &Screen) {
        if before.actions() != after.actions() {
            self.reset(after);
        }
    }

    /// Moves through available actions with wrapping for both keyboard and D-pad navigation.
    pub fn navigate(&mut self, screen: &Screen, backwards: bool) {
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
    pub fn press(&mut self, screen: &Screen, source: Source, hit: Option<Action>) {
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
    pub fn release(&mut self, source: Source, hit: Option<Action>) -> Option<Action> {
        let (action, captured) = self.capture?;
        if captured != source {
            return None;
        }
        self.capture = None;
        (source != Source::Pointer || hit == Some(action)).then_some(action)
    }

    /// The visible depressed face follows the pointer while a pointer press is held.
    pub fn pressed(&self, hovered: Option<Action>) -> Option<Action> {
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
#[derive(Clone, Copy)]
pub(super) enum Command {
    Navigate(bool),
    Press,
    Release,
    Cancel,
    Blur,
}

/// Converts gamepad button transitions without accepting held-button repeat events.
pub(super) fn controller(event: gilrs::EventType) -> Option<Command> {
    use gilrs::{Button, EventType};
    match event {
        EventType::ButtonPressed(Button::DPadLeft | Button::DPadUp, _) => {
            Some(Command::Navigate(true))
        }
        EventType::ButtonPressed(Button::DPadRight | Button::DPadDown, _) => {
            Some(Command::Navigate(false))
        }
        EventType::ButtonPressed(Button::South, _) => Some(Command::Press),
        EventType::ButtonReleased(Button::South, _) => Some(Command::Release),
        EventType::ButtonPressed(Button::East, _) => Some(Command::Cancel),
        EventType::Disconnected => Some(Command::Blur),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_and_controller_focus_wrap_and_activate_cancel() {
        for source in [Source::Keyboard, Source::Controller] {
            let mut input = Input::default();
            input.reset(&Screen::Consent);
            input.navigate(&Screen::Consent, true);
            assert_eq!(input.focused, Some(Action::Quit));
            assert!(input.focus_visible);
            input.navigate(&Screen::Consent, false);
            assert_eq!(input.focused, Some(Action::Accept));
            input.reset(&Screen::Starting);
            input.press(&Screen::Starting, source, None);
            assert_eq!(input.pressed(None), Some(Action::Quit));
            assert_eq!(input.release(source, None), Some(Action::Quit));
            assert_eq!(input.release(source, None), None);
        }
    }

    #[test]
    fn cancel_press_survives_download_and_unpack_reports() {
        let download = Screen::Downloading {
            received: 50,
            total: Some(100),
            bytes_per_second: None,
        };
        let unpack = Screen::Preparing {
            step: 1,
            total: 2,
            label: "Unpacking".into(),
        };
        for source in [Source::Keyboard, Source::Controller, Source::Pointer] {
            let mut input = Input::default();
            input.reset(&Screen::Starting);
            input.press(&Screen::Starting, source, Some(Action::Quit));
            input.update_screen(&Screen::Starting, &download);
            input.update_screen(&download, &unpack);
            assert_eq!(
                input.release(source, Some(Action::Quit)),
                Some(Action::Quit)
            );
            input.update_screen(
                &unpack,
                &Screen::Failed {
                    message: "offline".into(),
                },
            );
            assert_eq!(input.focused, Some(Action::Retry));
        }
    }

    #[test]
    fn pointer_drag_out_blur_and_screen_changes_cancel_presses() {
        let mut input = Input::default();
        input.reset(&Screen::Starting);
        input.press(&Screen::Starting, Source::Pointer, Some(Action::Quit));
        assert_eq!(input.pressed(None), None);
        assert_eq!(input.release(Source::Pointer, None), None);
        input.press(&Screen::Starting, Source::Keyboard, None);
        input.blur();
        assert_eq!(input.release(Source::Keyboard, None), None);
        input.press(&Screen::Starting, Source::Keyboard, None);
        input.reset(&Screen::Failed {
            message: "offline".into(),
        });
        assert_eq!(input.focused, Some(Action::Retry));
        assert_eq!(input.release(Source::Keyboard, None), None);
        input.reset(&Screen::Done);
        input.navigate(&Screen::Done, false);
        assert_eq!(input.focused, None);
    }

    #[test]
    fn retry_requires_matching_release_and_pointer_click_stays_hidden() {
        let screen = Screen::Failed {
            message: "offline".into(),
        };
        let mut input = Input::default();
        input.reset(&screen);
        input.press(&screen, Source::Pointer, Some(Action::Retry));
        assert!(!input.focus_visible);
        assert_eq!(input.release(Source::Keyboard, None), None);
        assert_eq!(
            input.release(Source::Pointer, Some(Action::Retry)),
            Some(Action::Retry)
        );
    }
}
