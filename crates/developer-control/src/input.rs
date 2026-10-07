//! Input commands reduced to held, released and tapped controls.

use crate::protocol::{InputCommand, Look, Pointer, Wheel};

/// Hotbar slots reachable through `key.hotbar.N`.
pub const HOTBAR_SLOTS: u8 = 9;
/// Default tap length: one rendered frame down, released on the next.
pub const DEFAULT_PRESS_FRAMES: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Control {
    /// A vanilla binding name resolved through the player's current key layout.
    Binding(String),
    /// A Bevy `KeyCode` debug name, as local mods bind keys.
    Key(String),
    Mouse(MouseButton),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl Control {
    pub fn parse(name: &str) -> Result<Self, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("empty control name".into());
        }
        if name.starts_with("key.") {
            return Ok(Self::Binding(name.to_owned()));
        }
        Ok(match name {
            "MouseLeft" => Self::Mouse(MouseButton::Left),
            "MouseRight" => Self::Mouse(MouseButton::Right),
            "MouseMiddle" => Self::Mouse(MouseButton::Middle),
            "MouseBack" => Self::Mouse(MouseButton::Back),
            "MouseForward" => Self::Mouse(MouseButton::Forward),
            _ if name.chars().all(|c| c.is_ascii_alphanumeric()) => Self::Key(name.to_owned()),
            _ => return Err(format!("unknown control `{name}`")),
        })
    }

    fn binding(name: &str) -> Self {
        Self::Binding(name.to_owned())
    }
}

/// What one input command changes, in application order: releases, then holds, then taps.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InputPlan {
    pub release_all: bool,
    pub release: Vec<Control>,
    pub hold: Vec<Control>,
    pub tap: Vec<Control>,
    pub tap_frames: u32,
    pub look: Option<Look>,
    pub text: Option<String>,
    pub pointer: Option<Pointer>,
    pub wheel: Option<Wheel>,
    pub release_control: bool,
}

impl InputPlan {
    pub fn from_command(command: &InputCommand) -> Result<Self, String> {
        let parse = |names: &[String]| -> Result<Vec<Control>, String> {
            names.iter().map(|name| Control::parse(name)).collect()
        };
        let mut plan = Self {
            release_all: command.release_all || command.release_control,
            release: parse(&command.release)?,
            hold: parse(&command.hold)?,
            tap: parse(&command.press)?,
            tap_frames: command.press_frames.unwrap_or(DEFAULT_PRESS_FRAMES).max(1),
            look: command.look,
            text: command.text.clone(),
            pointer: command
                .pointer
                .or_else(|| command.cursor.map(|[x, y]| Pointer { x, y })),
            wheel: command.wheel,
            release_control: command.release_control,
        };
        if let Some(look) = &command.look
            && !(look.yaw.is_finite() && look.pitch.is_finite())
        {
            return Err("look angles must be finite".into());
        }
        if command
            .cursor
            .is_some_and(|point| !point.into_iter().all(f32::is_finite))
        {
            return Err("cursor coordinates must be finite".into());
        }
        if let Some(pointer) = command.pointer
            && !(pointer.x.is_finite() && pointer.y.is_finite())
        {
            return Err("pointer coordinates must be finite".into());
        }
        if let Some(wheel) = command.wheel
            && !(wheel.x.is_finite() && wheel.y.is_finite())
        {
            return Err("wheel distances must be finite".into());
        }
        if let Some(movement) = command.movement {
            plan.axis(movement.forward, "key.forward", "key.back")?;
            plan.axis(movement.strafe, "key.right", "key.left")?;
        }
        for (state, name) in [
            (command.jump, "key.jump"),
            (command.sneak, "key.sneak"),
            (command.sprint, "key.sprint"),
        ] {
            match state {
                Some(true) => plan.hold.push(Control::binding(name)),
                Some(false) => plan.release.push(Control::binding(name)),
                None => {}
            }
        }
        if let Some(slot) = command.hotbar {
            if !(1..=HOTBAR_SLOTS).contains(&slot) {
                return Err(format!(
                    "hotbar slot must be 1..={HOTBAR_SLOTS}, got {slot}"
                ));
            }
            plan.tap
                .push(Control::Binding(format!("key.hotbar.{slot}")));
        }
        Ok(plan)
    }

    /// Holds `positive` or `negative` by the sign of `value`, releasing the other.
    fn axis(&mut self, value: f32, positive: &str, negative: &str) -> Result<(), String> {
        if !value.is_finite() {
            return Err("movement axes must be finite".into());
        }
        let (held, released): (&[&str], &[&str]) = if value > 0.0 {
            (&[positive], &[negative])
        } else if value < 0.0 {
            (&[negative], &[positive])
        } else {
            (&[], &[positive, negative])
        };
        self.hold
            .extend(held.iter().map(|name| Control::binding(name)));
        self.release
            .extend(released.iter().map(|name| Control::binding(name)));
        Ok(())
    }
}

/// Shortest signed turn from `from` to `to`, in degrees.
pub fn yaw_difference(from: f32, to: f32) -> f32 {
    (to - from + 180.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Move;

    #[test]
    fn controls_parse_by_kind() {
        assert_eq!(
            Control::parse("key.jump").unwrap(),
            Control::Binding("key.jump".into())
        );
        assert_eq!(
            Control::parse("Digit1").unwrap(),
            Control::Key("Digit1".into())
        );
        assert_eq!(
            Control::parse("MouseLeft").unwrap(),
            Control::Mouse(MouseButton::Left)
        );
        assert!(Control::parse("").is_err());
        assert!(Control::parse("Digit 1").is_err());
    }

    #[test]
    fn sugar_expands_to_bindings() {
        let command = InputCommand {
            movement: Some(Move {
                forward: 1.0,
                strafe: 0.0,
            }),
            jump: Some(true),
            sneak: Some(false),
            hotbar: Some(3),
            ..InputCommand::default()
        };
        let plan = InputPlan::from_command(&command).unwrap();
        let names = |controls: &[Control]| -> Vec<String> {
            controls
                .iter()
                .map(|control| match control {
                    Control::Binding(name) | Control::Key(name) => name.clone(),
                    Control::Mouse(button) => format!("{button:?}"),
                })
                .collect()
        };
        assert_eq!(names(&plan.hold), ["key.forward", "key.jump"]);
        assert_eq!(
            names(&plan.release),
            ["key.back", "key.right", "key.left", "key.sneak"]
        );
        assert_eq!(names(&plan.tap), ["key.hotbar.3"]);
        assert_eq!(plan.tap_frames, DEFAULT_PRESS_FRAMES);
    }

    #[test]
    fn invalid_input_is_rejected() {
        let slot = InputCommand {
            hotbar: Some(10),
            ..InputCommand::default()
        };
        assert!(InputPlan::from_command(&slot).is_err());
        let axis = InputCommand {
            movement: Some(Move {
                forward: f32::NAN,
                strafe: 0.0,
            }),
            ..InputCommand::default()
        };
        assert!(InputPlan::from_command(&axis).is_err());
        let control = InputCommand {
            press: vec!["not a key!".into()],
            ..InputCommand::default()
        };
        assert!(InputPlan::from_command(&control).is_err());
        let cursor = InputCommand {
            cursor: Some([f32::NAN, 10.0]),
            ..InputCommand::default()
        };
        assert!(InputPlan::from_command(&cursor).is_err());
    }

    #[test]
    fn release_control_releases_everything() {
        let plan = InputPlan::from_command(&InputCommand {
            release_control: true,
            ..InputCommand::default()
        })
        .unwrap();
        assert!(plan.release_all && plan.release_control);
    }

    #[test]
    fn pointer_and_wheel_validate_before_input_is_applied() {
        let mut command = InputCommand {
            cursor: Some([10.0, 20.0]),
            pointer: Some(Pointer { x: 32.5, y: 64.0 }),
            wheel: Some(Wheel {
                y: -3.0,
                ..Wheel::default()
            }),
            press: vec!["MouseLeft".into()],
            ..InputCommand::default()
        };
        let plan = InputPlan::from_command(&command).unwrap();
        assert_eq!(plan.pointer, command.pointer);
        assert_eq!(plan.wheel, command.wheel);
        assert_eq!(plan.tap, [Control::Mouse(MouseButton::Left)]);
        command.pointer.as_mut().unwrap().x = f32::NAN;
        assert!(InputPlan::from_command(&command).is_err());
        command.pointer = None;
        assert_eq!(
            InputPlan::from_command(&command).unwrap().pointer,
            Some(Pointer { x: 10.0, y: 20.0 })
        );
        command.wheel.as_mut().unwrap().y = f32::INFINITY;
        assert!(InputPlan::from_command(&command).is_err());
    }

    #[test]
    fn yaw_turns_the_short_way() {
        assert_eq!(yaw_difference(350.0, 10.0), 20.0);
        assert_eq!(yaw_difference(10.0, 350.0), -20.0);
        assert_eq!(yaw_difference(0.0, 90.0), 90.0);
    }
}
