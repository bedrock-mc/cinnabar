pub const MAX_CONTROLLERS: usize = 4;
pub const MAX_TOUCH_CONTACTS: usize = 16;
pub const MAX_KEYBOARD_KEYS: usize = 0xe7 - 0x04 + 1;
pub const MAX_MOUSE_BUTTONS: usize = 8;
pub const MAX_CONTROLLER_BUTTONS: usize = 32;
pub const MAX_DISCONNECTED_CONTROLLERS: usize = MAX_CONTROLLERS;
pub const MAX_TOUCH_CONTROLS: usize = 64;

use crate::ModifierChord;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyboardMouseFrame {
    /// Last change stamp from one translator-wide monotonically increasing counter.
    pub activity_sequence: u64,
    pub keys: Vec<u16>,
    pub mouse_buttons: Vec<u8>,
    pub mouse_motion: [f32; 2],
    pub modifiers: ModifierChord,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControllerFrame {
    pub device_id: u32,
    /// Last change stamp from the same global counter used by every device mode.
    pub activity_sequence: u64,
    /// Left X/Y, right X/Y, trigger axes, then two reserved portable axes.
    pub axes: [f32; 8],
    pub buttons: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TouchContact {
    pub contact_id: u64,
    /// Last change stamp from the same global counter used by every device mode.
    pub activity_sequence: u64,
    pub position: [f32; 2],
    pub delta: [f32; 2],
    pub hit_id: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TouchControlLayout {
    controls: Box<[u16]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchLayoutError {
    TooManyControls { actual: usize, maximum: usize },
    ZeroControlId,
    DuplicateControlId(u16),
}

impl TouchControlLayout {
    pub fn new(controls: Vec<u16>) -> Result<Self, TouchLayoutError> {
        if controls.len() > MAX_TOUCH_CONTROLS {
            return Err(TouchLayoutError::TooManyControls {
                actual: controls.len(),
                maximum: MAX_TOUCH_CONTROLS,
            });
        }
        if controls.contains(&0) {
            return Err(TouchLayoutError::ZeroControlId);
        }
        if let Some(hit_id) = first_duplicate(&controls) {
            return Err(TouchLayoutError::DuplicateControlId(hit_id));
        }
        Ok(Self {
            controls: controls.into_boxed_slice(),
        })
    }

    pub fn controls(&self) -> &[u16] {
        &self.controls
    }

    pub fn contains(&self, hit_id: u16) -> bool {
        self.controls.contains(&hit_id)
    }
}

impl Default for TouchControlLayout {
    fn default() -> Self {
        Self::new((1..=31).collect()).expect("built-in touch controls are valid")
    }
}

/// One bounded physical sample.
///
/// Every `activity_sequence` comes from a single translator-wide monotonic
/// change counter and retains its last value while that source is unchanged.
/// Equal stamps retain the current input mode; if it is absent, ties resolve
/// in `KeyboardMouse`, `GamePad`, then `Touch` order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceFrame {
    pub keyboard_mouse: Option<KeyboardMouseFrame>,
    pub controllers: Vec<ControllerFrame>,
    pub touches: Vec<TouchContact>,
    pub disconnected_controllers: Vec<u32>,
    pub window_focus_lost: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    TooManyControllers {
        actual: usize,
        maximum: usize,
    },
    TooManyTouches {
        actual: usize,
        maximum: usize,
    },
    TooManyKeyboardKeys {
        actual: usize,
        maximum: usize,
    },
    TooManyMouseButtons {
        actual: usize,
        maximum: usize,
    },
    TooManyControllerButtons {
        device_id: u32,
        actual: usize,
        maximum: usize,
    },
    TooManyDisconnectedControllers {
        actual: usize,
        maximum: usize,
    },
    DuplicateDeviceId(u32),
    DuplicateTouchContact(u64),
    DuplicateKeyboardUsage(u16),
    DuplicateMouseButton(u8),
    DuplicateControllerButton {
        device_id: u32,
        button: u8,
    },
    NonFiniteAxis,
    TouchCoordinateOutOfRange,
    UnknownPhysicalCode,
    UnknownTouchControl(u16),
}

impl DeviceFrame {
    pub(crate) fn validate(&self, touch_layout: &TouchControlLayout) -> Result<(), FrameError> {
        if self.controllers.len() > MAX_CONTROLLERS {
            return Err(FrameError::TooManyControllers {
                actual: self.controllers.len(),
                maximum: MAX_CONTROLLERS,
            });
        }
        if self.touches.len() > MAX_TOUCH_CONTACTS {
            return Err(FrameError::TooManyTouches {
                actual: self.touches.len(),
                maximum: MAX_TOUCH_CONTACTS,
            });
        }
        if let Some(keyboard) = &self.keyboard_mouse {
            if keyboard.keys.len() > MAX_KEYBOARD_KEYS {
                return Err(FrameError::TooManyKeyboardKeys {
                    actual: keyboard.keys.len(),
                    maximum: MAX_KEYBOARD_KEYS,
                });
            }
            if keyboard.mouse_buttons.len() > MAX_MOUSE_BUTTONS {
                return Err(FrameError::TooManyMouseButtons {
                    actual: keyboard.mouse_buttons.len(),
                    maximum: MAX_MOUSE_BUTTONS,
                });
            }
            if keyboard.mouse_motion.iter().any(|axis| !axis.is_finite()) {
                return Err(FrameError::NonFiniteAxis);
            }
            if keyboard
                .keys
                .iter()
                .any(|code| !(0x04..=0xe7).contains(code))
                || keyboard
                    .mouse_buttons
                    .iter()
                    .any(|button| !(1..=8).contains(button))
            {
                return Err(FrameError::UnknownPhysicalCode);
            }
            if let Some(code) = first_duplicate(&keyboard.keys) {
                return Err(FrameError::DuplicateKeyboardUsage(code));
            }
            if let Some(button) = first_duplicate(&keyboard.mouse_buttons) {
                return Err(FrameError::DuplicateMouseButton(button));
            }
        }
        for (index, controller) in self.controllers.iter().enumerate() {
            if self.controllers[..index]
                .iter()
                .any(|other| other.device_id == controller.device_id)
            {
                return Err(FrameError::DuplicateDeviceId(controller.device_id));
            }
            if controller.axes.iter().any(|axis| !axis.is_finite()) {
                return Err(FrameError::NonFiniteAxis);
            }
            if controller.buttons.len() > MAX_CONTROLLER_BUTTONS {
                return Err(FrameError::TooManyControllerButtons {
                    device_id: controller.device_id,
                    actual: controller.buttons.len(),
                    maximum: MAX_CONTROLLER_BUTTONS,
                });
            }
            if controller.buttons.iter().any(|button| *button > 31) {
                return Err(FrameError::UnknownPhysicalCode);
            }
            if let Some(button) = first_duplicate(&controller.buttons) {
                return Err(FrameError::DuplicateControllerButton {
                    device_id: controller.device_id,
                    button,
                });
            }
        }
        for (index, contact) in self.touches.iter().enumerate() {
            if self.touches[..index]
                .iter()
                .any(|other| other.contact_id == contact.contact_id)
            {
                return Err(FrameError::DuplicateTouchContact(contact.contact_id));
            }
            if contact
                .position
                .iter()
                .chain(contact.delta.iter())
                .any(|axis| !axis.is_finite())
            {
                return Err(FrameError::NonFiniteAxis);
            }
            if contact
                .position
                .iter()
                .any(|axis| !(0.0..=1.0).contains(axis))
            {
                return Err(FrameError::TouchCoordinateOutOfRange);
            }
            if let Some(hit_id) = contact.hit_id
                && !touch_layout.contains(hit_id)
            {
                return Err(FrameError::UnknownTouchControl(hit_id));
            }
        }
        if self.disconnected_controllers.len() > MAX_DISCONNECTED_CONTROLLERS {
            return Err(FrameError::TooManyDisconnectedControllers {
                actual: self.disconnected_controllers.len(),
                maximum: MAX_DISCONNECTED_CONTROLLERS,
            });
        }
        for (index, device_id) in self.disconnected_controllers.iter().enumerate() {
            if self.disconnected_controllers[..index].contains(device_id)
                || self
                    .controllers
                    .iter()
                    .any(|controller| controller.device_id == *device_id)
            {
                return Err(FrameError::DuplicateDeviceId(*device_id));
            }
        }
        Ok(())
    }
}

fn first_duplicate<T: Copy + PartialEq>(values: &[T]) -> Option<T> {
    values
        .iter()
        .enumerate()
        .find_map(|(index, value)| values[..index].contains(value).then_some(*value))
}
