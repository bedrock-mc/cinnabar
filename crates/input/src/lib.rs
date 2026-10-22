//! Vendor-independent, renderer-independent semantic input primitives.

mod action;
mod click_limit;
pub use click_limit::ClickLimiter;
mod axes;
mod binding;
mod button_events;
mod desktop;
mod device;
pub mod finalized;
pub mod identity;
mod ordered;
mod router;

pub use action::{
    Action, ActionPhase, ActionSnapshot, InputContext, InputMode, MovementButtons, PerspectiveMode,
    ReleaseReason,
};
pub use axes::clamp_movement_vector;
pub use binding::{
    ActionBinding, AxisDirection, BindingError, ControlSettings, DEFAULT_MOUSE_SENSITIVITY,
    InputChord, MAX_BINDINGS, ModifierChord, MouseAxis, PhysicalControl,
};
pub use desktop::{KEYBOARD_USAGES, KeyboardKey, ScrollUnit};
pub use device::{
    ButtonEdges, ControllerFrame, DeviceFrame, FrameError, KeyboardMouseFrame,
    MAX_CONTROLLER_BUTTONS, MAX_CONTROLLERS, MAX_DISCONNECTED_CONTROLLERS, MAX_KEYBOARD_KEYS,
    MAX_MOUSE_BUTTONS, MAX_TOUCH_CONTACTS, MAX_TOUCH_CONTROLS, TouchAxis, TouchContact,
    TouchControl, TouchControlKind, TouchControlLayout, TouchLayoutError,
};
pub use ordered::{
    ActionButtons, ActionEvent, DesktopEvent, DesktopEvents, MAX_PENDING_ACTION_EVENTS,
    OrderedActions,
};
pub use router::{MAX_LOOK_DELTA_PER_FRAME, RouterError, SemanticInputRouter};

/// Stable default hit identifiers supplied by the app-owned touch layout.
pub mod touch {
    pub const JUMP: u16 = 1;
    pub const SNEAK: u16 = 2;
    pub const SPRINT: u16 = 3;
    pub const ATTACK: u16 = 4;
    pub const USE: u16 = 5;
    pub const PERSPECTIVE: u16 = 6;
    pub const MENU: u16 = 7;
    pub const BACK: u16 = 8;
    pub const HOTBAR_1: u16 = 9;
    pub const HOTBAR_2: u16 = 10;
    pub const HOTBAR_3: u16 = 11;
    pub const HOTBAR_4: u16 = 12;
    pub const HOTBAR_5: u16 = 13;
    pub const HOTBAR_6: u16 = 14;
    pub const HOTBAR_7: u16 = 15;
    pub const HOTBAR_8: u16 = 16;
    pub const HOTBAR_9: u16 = 17;
    pub const HOTBAR_PREVIOUS: u16 = 18;
    pub const HOTBAR_NEXT: u16 = 19;
    pub const UI_UP: u16 = 20;
    pub const UI_DOWN: u16 = 21;
    pub const UI_LEFT: u16 = 22;
    pub const UI_RIGHT: u16 = 23;
    pub const UI_ACCEPT: u16 = 24;
    pub const UI_CANCEL: u16 = 25;
    pub const UI_TAB_NEXT: u16 = 26;
    pub const UI_TAB_PREVIOUS: u16 = 27;
    pub const LOOK_UP: u16 = 28;
    pub const LOOK_DOWN: u16 = 29;
    pub const LOOK_LEFT: u16 = 30;
    pub const LOOK_RIGHT: u16 = 31;
    /// Drags anywhere outside another control; the contact delta drives all four look axes.
    pub const LOOK_SURFACE: u16 = 32;
    /// The movement stick; the contact delta is its normalized deflection, positive Y forward.
    pub const JOYSTICK: u16 = 33;
    pub const INVENTORY: u16 = 34;
    pub const CHAT: u16 = 35;
    pub const EMOTE: u16 = 36;
}

mod touch_controls;
pub use touch_controls::{TouchBounds, TouchControlRegion, TouchControlState};

pub mod cursor_focus;

/// Hotbar digit actions in slot order.
pub const HOTBAR_DIGIT_ACTIONS: &[Action] = &[
    Action::Hotbar1,
    Action::Hotbar2,
    Action::Hotbar3,
    Action::Hotbar4,
    Action::Hotbar5,
    Action::Hotbar6,
    Action::Hotbar7,
    Action::Hotbar8,
    Action::Hotbar9,
];
