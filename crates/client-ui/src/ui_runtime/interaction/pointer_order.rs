//! Routes a frame's mouse presses against the screen state in force when each arrived.

use bevy::{input::ButtonState, prelude::MouseButton, window::WindowEvent};

/// This frame's mouse presses in arrival order, each with the number of keyboard
/// events that reached the window before it.
pub fn ordered_pointer_presses<'a>(
    events: impl IntoIterator<Item = &'a WindowEvent>,
) -> Vec<(usize, MouseButton)> {
    // Every event is consumed so the caller's cursor never replays this frame's keys.
    let mut keys = 0;
    let mut presses = Vec::new();
    for event in events {
        match event {
            WindowEvent::KeyboardInput(_) => keys += 1,
            WindowEvent::MouseButtonInput(input) if input.state == ButtonState::Pressed => {
                presses.push((keys, input.button));
            }
            _ => {}
        }
    }
    presses
}

/// Where a press went: a screen binding it triggered, gameplay, or the screen opening that was
/// showing when it arrived (0 is the screen already open at the start of the frame).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressRoute {
    Binding,
    Gameplay,
    Screen { opening: usize },
}

/// Hands each press out once, in arrival order, for the caller to route against the screen
/// state in force at that point.
#[derive(Debug, Default)]
pub struct PointerRouter {
    presses: Vec<(usize, MouseButton)>,
    next: usize,
    open: bool,
    opening: usize,
}

impl PointerRouter {
    pub fn new(presses: Vec<(usize, MouseButton)>, open_at_start: bool) -> Self {
        Self {
            presses,
            next: 0,
            open: open_at_start,
            opening: 0,
        }
    }

    /// The next press that arrived before keyboard event `arrival`, if any is left.
    pub fn next_before_key(&mut self, arrival: usize) -> Option<MouseButton> {
        let (keys, button) = *self.presses.get(self.next)?;
        (keys <= arrival).then(|| {
            self.next += 1;
            button
        })
    }

    /// The next press left after the frame's last keyboard event.
    pub fn next_rest(&mut self) -> Option<MouseButton> {
        self.next_before_key(usize::MAX)
    }

    /// Routes the press just taken to gameplay or the screen `open` reports; a screen that was
    /// closed since the last press counts as a new opening.
    pub fn route(&mut self, open: bool) -> PressRoute {
        if open && !self.open {
            self.opening += 1;
        }
        self.open = open;
        if open {
            PressRoute::Screen {
                opening: self.opening,
            }
        } else {
            PressRoute::Gameplay
        }
    }
}

/// How a key or binding press changes the screen in [`route_frame_presses`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenKey {
    Toggle,
    Close,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameInput {
    Press(MouseButton),
    /// A press bound to a screen toggle: it toggles the screen and is never also a click.
    BindingPress(MouseButton),
    Key(ScreenKey),
}

/// Routes one frame's ordered inputs: every press goes once, to the state in force when it arrived.
pub fn route_frame_presses(
    inputs: &[FrameInput],
    open_at_start: bool,
) -> Vec<(MouseButton, PressRoute)> {
    let mut keys = 0;
    let mut presses = Vec::new();
    let mut bindings = Vec::new();
    for input in inputs {
        match input {
            FrameInput::Press(button) | FrameInput::BindingPress(button) => {
                presses.push((keys, *button));
                bindings.push(matches!(input, FrameInput::BindingPress(_)));
            }
            FrameInput::Key(_) => keys += 1,
        }
    }
    let mut router = PointerRouter::new(presses, open_at_start);
    let mut bindings = bindings.into_iter();
    let mut open = open_at_start;
    let mut routed = Vec::new();
    let mut route = |router: &mut PointerRouter, open: &mut bool, button| {
        if bindings.next() == Some(true) {
            *open = !*open;
            routed.push((button, PressRoute::Binding));
        } else {
            routed.push((button, router.route(*open)));
        }
    };
    let screen_keys = inputs.iter().filter_map(|input| match input {
        FrameInput::Key(key) => Some(*key),
        FrameInput::Press(_) | FrameInput::BindingPress(_) => None,
    });
    for (arrival, key) in screen_keys.enumerate() {
        while let Some(button) = router.next_before_key(arrival) {
            route(&mut router, &mut open, button);
        }
        open = match key {
            ScreenKey::Toggle => !open,
            ScreenKey::Close => false,
            ScreenKey::Other => open,
        };
    }
    while let Some(button) = router.next_rest() {
        route(&mut router, &mut open, button);
    }
    routed
}

#[cfg(test)]
mod tests {
    use super::{FrameInput::*, PressRoute::*, ScreenKey::*, *};

    const LEFT: FrameInput = Press(MouseButton::Left);
    const BIND: FrameInput = BindingPress(MouseButton::Right);
    const RIGHT: FrameInput = Press(MouseButton::Right);
    const OPEN: FrameInput = Key(Toggle);
    const ESCAPE: FrameInput = Key(Close);
    const OTHER: FrameInput = Key(Other);

    #[test]
    fn each_press_is_routed_once_to_the_state_in_force_when_it_arrived() {
        let first = Screen { opening: 0 };
        let opened = Screen { opening: 1 };
        let cases: &[(&str, bool, &[FrameInput], &[(MouseButton, PressRoute)])] = &[
            (
                "click then Escape",
                true,
                &[LEFT, ESCAPE],
                &[(MouseButton::Left, first)],
            ),
            (
                "Escape then click",
                true,
                &[ESCAPE, LEFT],
                &[(MouseButton::Left, Gameplay)],
            ),
            (
                "click, Escape, T, Q",
                true,
                &[LEFT, ESCAPE, OTHER, OTHER],
                &[(MouseButton::Left, first)],
            ),
            (
                "right-click, Escape, left-click",
                true,
                &[RIGHT, ESCAPE, LEFT],
                &[(MouseButton::Right, first), (MouseButton::Left, Gameplay)],
            ),
            (
                "press, open, Escape",
                false,
                &[LEFT, OPEN, ESCAPE],
                &[(MouseButton::Left, Gameplay)],
            ),
            (
                "open, press, Escape, open, Escape",
                false,
                &[OPEN, LEFT, ESCAPE, OPEN, ESCAPE],
                &[(MouseButton::Left, opened)],
            ),
            (
                "binding right-click opens, then Q",
                false,
                &[BIND, OTHER],
                &[(MouseButton::Right, Binding)],
            ),
            (
                "binding opens, then click, then Escape",
                false,
                &[BIND, LEFT, ESCAPE],
                &[(MouseButton::Right, Binding), (MouseButton::Left, opened)],
            ),
            (
                "click, then binding closes",
                true,
                &[LEFT, BIND, OTHER],
                &[(MouseButton::Left, first), (MouseButton::Right, Binding)],
            ),
            (
                "close, reopen, then press",
                true,
                &[ESCAPE, OPEN, LEFT],
                &[(MouseButton::Left, opened)],
            ),
        ];
        for (name, open, inputs, expected) in cases {
            assert_eq!(&route_frame_presses(inputs, *open), expected, "{name}");
        }
    }

    #[test]
    fn window_scan_counts_keys_before_each_press_and_reads_everything() {
        use bevy::{
            ecs::entity::Entity,
            input::{
                keyboard::{Key, KeyCode, KeyboardInput},
                mouse::MouseButtonInput,
            },
        };
        let window = Entity::PLACEHOLDER;
        let key = WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
        let press = |button| {
            WindowEvent::MouseButtonInput(MouseButtonInput {
                button,
                state: ButtonState::Pressed,
                window,
            })
        };
        let events = [
            press(MouseButton::Right),
            key.clone(),
            press(MouseButton::Left),
            key,
        ];
        let mut iter = events.iter();
        assert_eq!(
            ordered_pointer_presses(&mut iter),
            [(0, MouseButton::Right), (1, MouseButton::Left)]
        );
        assert!(iter.next().is_none());
    }
}
