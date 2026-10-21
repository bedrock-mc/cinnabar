use crate::identity::{InputAuthorityGeneration, InputSequence};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u8)]
pub enum Action {
    MoveForward,
    MoveBackward,
    MoveLeft,
    MoveRight,
    LookUp,
    LookDown,
    LookLeft,
    LookRight,
    Jump,
    Sneak,
    Sprint,
    Attack,
    Use,
    CyclePerspective,
    Menu,
    Back,
    Hotbar1,
    Hotbar2,
    Hotbar3,
    Hotbar4,
    Hotbar5,
    Hotbar6,
    Hotbar7,
    Hotbar8,
    Hotbar9,
    HotbarPrevious,
    HotbarNext,
    UiUp,
    UiDown,
    UiLeft,
    UiRight,
    UiAccept,
    UiCancel,
    UiTabNext,
    UiTabPrevious,
    PlayerList,
    Freelook,
    /// Vanilla's "Open Notification": opens what the showing toast offers.
    InteractWithToast,
    PickItem,
}

impl Action {
    pub const COUNT: usize = 39;

    pub(crate) const ALL: [Self; Self::COUNT] = [
        Self::MoveForward,
        Self::MoveBackward,
        Self::MoveLeft,
        Self::MoveRight,
        Self::LookUp,
        Self::LookDown,
        Self::LookLeft,
        Self::LookRight,
        Self::Jump,
        Self::Sneak,
        Self::Sprint,
        Self::Attack,
        Self::Use,
        Self::CyclePerspective,
        Self::Menu,
        Self::Back,
        Self::Hotbar1,
        Self::Hotbar2,
        Self::Hotbar3,
        Self::Hotbar4,
        Self::Hotbar5,
        Self::Hotbar6,
        Self::Hotbar7,
        Self::Hotbar8,
        Self::Hotbar9,
        Self::HotbarPrevious,
        Self::HotbarNext,
        Self::UiUp,
        Self::UiDown,
        Self::UiLeft,
        Self::UiRight,
        Self::UiAccept,
        Self::UiCancel,
        Self::UiTabNext,
        Self::UiTabPrevious,
        Self::PlayerList,
        Self::Freelook,
        Self::InteractWithToast,
        Self::PickItem,
    ];

    pub(crate) const fn is_ui_preview(self) -> bool {
        matches!(
            self,
            Self::Menu
                | Self::Back
                | Self::UiUp
                | Self::UiDown
                | Self::UiLeft
                | Self::UiRight
                | Self::UiAccept
                | Self::UiCancel
                | Self::UiTabNext
                | Self::UiTabPrevious
        )
    }

    pub(crate) const fn is_one_shot(self) -> bool {
        matches!(
            self,
            Self::CyclePerspective
                | Self::PickItem
                | Self::InteractWithToast
                | Self::Menu
                | Self::Back
                | Self::Hotbar1
                | Self::Hotbar2
                | Self::Hotbar3
                | Self::Hotbar4
                | Self::Hotbar5
                | Self::Hotbar6
                | Self::Hotbar7
                | Self::Hotbar8
                | Self::Hotbar9
                | Self::HotbarPrevious
                | Self::HotbarNext
                | Self::UiUp
                | Self::UiDown
                | Self::UiLeft
                | Self::UiRight
                | Self::UiAccept
                | Self::UiCancel
                | Self::UiTabNext
                | Self::UiTabPrevious
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionPhase {
    pub pressed: bool,
    pub held: bool,
    pub released: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum InputMode {
    #[default]
    KeyboardMouse,
    GamePad,
    Touch,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PerspectiveMode {
    #[default]
    FirstPerson,
    ThirdPersonBack,
    ThirdPersonFront,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum InputContext {
    #[default]
    Gameplay,
    UiFocused,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReleaseReason {
    WindowFocusLost,
    UiFocusTaken,
    ControllerDisconnected,
    SessionReplaced,
    DimensionReplaced,
    AuthorityChanged,
    BindingChanged,
    InputOverflow,
}

impl ReleaseReason {
    pub(crate) const fn priority(self) -> u8 {
        match self {
            Self::SessionReplaced => 8,
            Self::DimensionReplaced => 7,
            Self::AuthorityChanged => 6,
            Self::BindingChanged => 5,
            Self::InputOverflow => 4,
            Self::ControllerDisconnected => 3,
            Self::WindowFocusLost => 2,
            Self::UiFocusTaken => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionSnapshot {
    /// Ordered desktop transitions, when supplied by the physical adapter.
    pub ordered: Option<crate::OrderedActions>,
    pub frame_sequence: InputSequence,
    pub authority_generation: InputAuthorityGeneration,
    /// Unit-clamped movement consumed by simulation and packet construction.
    pub movement: [f32; 2],
    /// Controlling-device differential before magnitude normalization.
    ///
    /// The controlling device is [`Self::input_mode`]: one deterministic
    /// per-frame selection (latest activity sequence wins, ties retain the
    /// active mode, fixed fallback order otherwise), so exactly one device
    /// class contributes to these carriers.
    pub raw_movement: [f32; 2],
    /// Analog-axis contribution of the controlling device (post-deadzone
    /// gamepad axes). Keyboard buttons leave this vector empty.
    pub analogue_movement: [f32; 2],
    /// Digital direction buttons, independent of analogue axes and opposing-key cancellation.
    pub movement_buttons: MovementButtons,
    pub look_delta: [f32; 2],
    pub input_mode: InputMode,
    pub phases: [ActionPhase; Action::COUNT],
    pub release_reasons: [Option<ReleaseReason>; Action::COUNT],
    /// An input authority boundary occurred, including when no action was held.
    pub authority_released: bool,
}

/// Direction buttons retain their identities even when their resulting vector is zero.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MovementButtons {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
}

impl MovementButtons {
    /// Records only movement actions supplied by a digital control.
    pub(crate) fn set(&mut self, action: Action) {
        match action {
            Action::MoveForward => self.forward = true,
            Action::MoveBackward => self.backward = true,
            Action::MoveLeft => self.left = true,
            Action::MoveRight => self.right = true,
            _ => {}
        }
    }
}
