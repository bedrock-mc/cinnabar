pub const MAX_CONTROLLERS: usize = 4;
pub const MAX_TOUCH_CONTACTS: usize = 16;

use crate::ModifierChord;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyboardMouseFrame {
    pub activity_sequence: u64,
    pub keys: Vec<u16>,
    pub mouse_buttons: Vec<u8>,
    pub mouse_motion: [f32; 2],
    pub modifiers: ModifierChord,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControllerFrame {
    pub device_id: u32,
    pub activity_sequence: u64,
    /// Left X/Y, right X/Y, trigger axes, then two reserved portable axes.
    pub axes: [f32; 8],
    pub buttons: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TouchContact {
    pub contact_id: u64,
    pub activity_sequence: u64,
    pub position: [f32; 2],
    pub delta: [f32; 2],
    pub hit_id: Option<u16>,
}

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
    TooManyControllers { actual: usize, maximum: usize },
    TooManyTouches { actual: usize, maximum: usize },
    DuplicateDeviceId(u32),
    DuplicateTouchContact(u64),
    NonFiniteAxis,
    TouchCoordinateOutOfRange,
    UnknownPhysicalCode,
}

impl DeviceFrame {
    pub(crate) fn validate(&self) -> Result<(), FrameError> {
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
            if controller.buttons.iter().any(|button| *button > 31) {
                return Err(FrameError::UnknownPhysicalCode);
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
            if contact.hit_id == Some(0) {
                return Err(FrameError::UnknownPhysicalCode);
            }
        }
        for (index, device_id) in self.disconnected_controllers.iter().enumerate() {
            if self.disconnected_controllers[..index].contains(device_id) {
                return Err(FrameError::DuplicateDeviceId(*device_id));
            }
        }
        Ok(())
    }
}
