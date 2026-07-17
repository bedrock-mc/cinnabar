//! Vendor-independent, renderer-independent semantic input primitives.

mod action;
mod binding;
mod device;
mod router;

pub use action::{
    Action, ActionPhase, ActionSnapshot, InputContext, InputMode, PerspectiveMode, ReleaseReason,
};
pub use binding::{
    ActionBinding, AxisDirection, BindingError, ControlSettings, InputChord, MAX_BINDINGS,
    ModifierChord, MouseAxis, PhysicalControl,
};
pub use device::{
    ControllerFrame, DeviceFrame, FrameError, KeyboardMouseFrame, MAX_CONTROLLERS,
    MAX_TOUCH_CONTACTS, TouchContact,
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
}
