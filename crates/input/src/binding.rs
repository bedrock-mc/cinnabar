use crate::{Action, InputContext, touch};

pub const MAX_BINDINGS: usize = 128;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MouseAxis {
    XPositive,
    XNegative,
    YPositive,
    YNegative,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AxisDirection {
    Positive,
    Negative,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PhysicalControl {
    KeyboardUsage(u16),
    MouseButton(u8),
    MouseAxis(MouseAxis),
    GamepadButton(u8),
    GamepadAxis { axis: u8, direction: AxisDirection },
    TouchControl(u16),
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ModifierChord {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl ModifierChord {
    pub(crate) const fn is_satisfied_by(self, actual: Self) -> bool {
        (!self.shift || actual.shift)
            && (!self.control || actual.control)
            && (!self.alt || actual.alt)
            && (!self.super_key || actual.super_key)
    }

    pub(crate) const fn specificity(self) -> u8 {
        self.shift as u8 + self.control as u8 + self.alt as u8 + self.super_key as u8
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct InputChord {
    pub control: PhysicalControl,
    pub modifiers: ModifierChord,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ActionBinding {
    pub action: Action,
    pub context: InputContext,
    pub chord: InputChord,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlSettings {
    bindings: Box<[ActionBinding]>,
    pub mouse_sensitivity: f32,
    pub gamepad_look_sensitivity: f32,
    pub touch_look_sensitivity: f32,
    pub invert_mouse_y: bool,
    pub invert_gamepad_y: bool,
    pub gamepad_move_deadzone: f32,
    pub gamepad_look_deadzone: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingError {
    TooManyBindings {
        actual: usize,
        maximum: usize,
    },
    NonFiniteSensitivity,
    SensitivityOutOfRange,
    NonFiniteDeadzone,
    DeadzoneOutOfRange,
    UnknownPhysicalCode,
    Conflict {
        context: InputContext,
        chord: InputChord,
        first: Action,
        second: Action,
    },
}

impl ControlSettings {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bindings: Vec<ActionBinding>,
        mouse_sensitivity: f32,
        gamepad_look_sensitivity: f32,
        touch_look_sensitivity: f32,
        invert_mouse_y: bool,
        invert_gamepad_y: bool,
        gamepad_move_deadzone: f32,
        gamepad_look_deadzone: f32,
    ) -> Result<Self, BindingError> {
        let settings = Self {
            bindings: bindings.into_boxed_slice(),
            mouse_sensitivity,
            gamepad_look_sensitivity,
            touch_look_sensitivity,
            invert_mouse_y,
            invert_gamepad_y,
            gamepad_move_deadzone,
            gamepad_look_deadzone,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn bindings(&self) -> &[ActionBinding] {
        &self.bindings
    }

    pub(crate) fn validate(&self) -> Result<(), BindingError> {
        if self.bindings.len() > MAX_BINDINGS {
            return Err(BindingError::TooManyBindings {
                actual: self.bindings.len(),
                maximum: MAX_BINDINGS,
            });
        }
        let sensitivities = [
            self.mouse_sensitivity,
            self.gamepad_look_sensitivity,
            self.touch_look_sensitivity,
        ];
        if sensitivities.iter().any(|value| !value.is_finite()) {
            return Err(BindingError::NonFiniteSensitivity);
        }
        if sensitivities
            .iter()
            .any(|value| !(0.01..=10.0).contains(value))
        {
            return Err(BindingError::SensitivityOutOfRange);
        }
        let deadzones = [self.gamepad_move_deadzone, self.gamepad_look_deadzone];
        if deadzones.iter().any(|value| !value.is_finite()) {
            return Err(BindingError::NonFiniteDeadzone);
        }
        if deadzones.iter().any(|value| !(0.0..=0.95).contains(value)) {
            return Err(BindingError::DeadzoneOutOfRange);
        }
        for (index, binding) in self.bindings.iter().enumerate() {
            validate_control(binding.chord.control)?;
            if let Some(first) = self.bindings[..index].iter().find(|candidate| {
                candidate.context == binding.context && candidate.chord == binding.chord
            }) {
                return Err(BindingError::Conflict {
                    context: binding.context,
                    chord: binding.chord,
                    first: first.action,
                    second: binding.action,
                });
            }
        }
        Ok(())
    }
}

impl Default for ControlSettings {
    fn default() -> Self {
        Self::new(default_bindings(), 1.0, 1.0, 1.0, false, false, 0.15, 0.15)
            .expect("built-in controls are valid")
    }
}

fn validate_control(control: PhysicalControl) -> Result<(), BindingError> {
    let valid = match control {
        PhysicalControl::KeyboardUsage(code) => (0x04..=0xe7).contains(&code),
        PhysicalControl::MouseButton(button) => (1..=8).contains(&button),
        PhysicalControl::MouseAxis(_) => true,
        PhysicalControl::GamepadButton(button) => button <= 31,
        PhysicalControl::GamepadAxis { axis, .. } => axis <= 7,
        PhysicalControl::TouchControl(hit_id) => hit_id != 0,
    };
    valid.then_some(()).ok_or(BindingError::UnknownPhysicalCode)
}

fn chord(control: PhysicalControl) -> InputChord {
    InputChord {
        control,
        modifiers: ModifierChord::default(),
    }
}

fn bind(
    bindings: &mut Vec<ActionBinding>,
    action: Action,
    context: InputContext,
    control: PhysicalControl,
) {
    bindings.push(ActionBinding {
        action,
        context,
        chord: chord(control),
    });
}

fn default_bindings() -> Vec<ActionBinding> {
    use self::MouseAxis::{XNegative, XPositive, YNegative, YPositive};
    use Action::*;
    use AxisDirection::{Negative, Positive};
    use InputContext::{Gameplay, UiFocused};
    use PhysicalControl::{
        GamepadAxis, GamepadButton, KeyboardUsage, MouseAxis, MouseButton, TouchControl,
    };

    let mut bindings = Vec::new();
    for (action, code) in [
        (MoveForward, 0x1a),
        (MoveBackward, 0x16),
        (MoveLeft, 0x04),
        (MoveRight, 0x07),
        (Jump, 0x2c),
        (Sneak, 0xe1),
        (Sneak, 0xe5),
        (Sprint, 0xe0),
        (Sprint, 0xe4),
        (CyclePerspective, 0x3e),
        (Menu, 0x29),
        (Hotbar1, 0x1e),
        (Hotbar2, 0x1f),
        (Hotbar3, 0x20),
        (Hotbar4, 0x21),
        (Hotbar5, 0x22),
        (Hotbar6, 0x23),
        (Hotbar7, 0x24),
        (Hotbar8, 0x25),
        (Hotbar9, 0x26),
    ] {
        bind(&mut bindings, action, Gameplay, KeyboardUsage(code));
    }
    bind(&mut bindings, Attack, Gameplay, MouseButton(1));
    bind(&mut bindings, Use, Gameplay, MouseButton(2));
    for (action, axis) in [
        (LookRight, XPositive),
        (LookLeft, XNegative),
        (LookDown, YPositive),
        (LookUp, YNegative),
    ] {
        bind(&mut bindings, action, Gameplay, MouseAxis(axis));
    }
    for (action, code) in [
        (UiUp, 0x52),
        (UiDown, 0x51),
        (UiLeft, 0x50),
        (UiRight, 0x4f),
        (UiAccept, 0x28),
        (Back, 0x29),
        (UiTabNext, 0x2b),
    ] {
        bind(&mut bindings, action, UiFocused, KeyboardUsage(code));
    }
    bindings.push(ActionBinding {
        action: UiTabPrevious,
        context: UiFocused,
        chord: InputChord {
            control: KeyboardUsage(0x2b),
            modifiers: ModifierChord {
                shift: true,
                ..ModifierChord::default()
            },
        },
    });

    for (action, axis, direction) in [
        (MoveRight, 0, Positive),
        (MoveLeft, 0, Negative),
        (MoveForward, 1, Positive),
        (MoveBackward, 1, Negative),
        (LookRight, 2, Positive),
        (LookLeft, 2, Negative),
        (LookUp, 3, Positive),
        (LookDown, 3, Negative),
        (Attack, 4, Positive),
        (Use, 5, Positive),
    ] {
        bind(
            &mut bindings,
            action,
            Gameplay,
            GamepadAxis { axis, direction },
        );
    }
    for (action, button) in [
        (Jump, 0),
        (Sneak, 1),
        (HotbarPrevious, 4),
        (HotbarNext, 5),
        (Back, 6),
        (Menu, 7),
        (Sprint, 8),
        (CyclePerspective, 11),
    ] {
        bind(&mut bindings, action, Gameplay, GamepadButton(button));
    }
    for (action, button) in [
        (UiAccept, 0),
        (UiCancel, 1),
        (UiTabPrevious, 4),
        (UiTabNext, 5),
        (Back, 6),
        (Menu, 7),
        (UiUp, 11),
        (UiDown, 12),
        (UiLeft, 13),
        (UiRight, 14),
    ] {
        bind(&mut bindings, action, UiFocused, GamepadButton(button));
    }

    for (action, hit_id, context) in [
        (Jump, touch::JUMP, Gameplay),
        (Sneak, touch::SNEAK, Gameplay),
        (Sprint, touch::SPRINT, Gameplay),
        (Attack, touch::ATTACK, Gameplay),
        (Use, touch::USE, Gameplay),
        (CyclePerspective, touch::PERSPECTIVE, Gameplay),
        (Menu, touch::MENU, Gameplay),
        (Back, touch::BACK, UiFocused),
        (Hotbar1, touch::HOTBAR_1, Gameplay),
        (Hotbar2, touch::HOTBAR_2, Gameplay),
        (Hotbar3, touch::HOTBAR_3, Gameplay),
        (Hotbar4, touch::HOTBAR_4, Gameplay),
        (Hotbar5, touch::HOTBAR_5, Gameplay),
        (Hotbar6, touch::HOTBAR_6, Gameplay),
        (Hotbar7, touch::HOTBAR_7, Gameplay),
        (Hotbar8, touch::HOTBAR_8, Gameplay),
        (Hotbar9, touch::HOTBAR_9, Gameplay),
        (HotbarPrevious, touch::HOTBAR_PREVIOUS, Gameplay),
        (HotbarNext, touch::HOTBAR_NEXT, Gameplay),
        (UiUp, touch::UI_UP, UiFocused),
        (UiDown, touch::UI_DOWN, UiFocused),
        (UiLeft, touch::UI_LEFT, UiFocused),
        (UiRight, touch::UI_RIGHT, UiFocused),
        (UiAccept, touch::UI_ACCEPT, UiFocused),
        (UiCancel, touch::UI_CANCEL, UiFocused),
        (UiTabNext, touch::UI_TAB_NEXT, UiFocused),
        (UiTabPrevious, touch::UI_TAB_PREVIOUS, UiFocused),
    ] {
        bind(&mut bindings, action, context, TouchControl(hit_id));
    }
    bindings
}
