//! Movement bobbing shared by settings hosts and first-person presentation.

/// Saved values preserve the original Off/On preferences as Off/Full.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(i32)]
pub enum ViewBobbing {
    Off = 0,
    #[default]
    Full = 1,
    Minimal = 2,
}

impl ViewBobbing {
    pub const ALL: [Self; 3] = [Self::Off, Self::Full, Self::Minimal];

    /// Decodes a saved choice, retaining the default for unknown values.
    pub const fn from_value(value: i32) -> Self {
        match value {
            0 => Self::Off,
            2 => Self::Minimal,
            _ => Self::Full,
        }
    }

    /// Labels shared by the Video settings hosts.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Full => "Full",
            Self::Minimal => "Minimal",
        }
    }

    /// Whether walking and authored first-person hand bobbing remain enabled.
    pub const fn hand_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether movement bobbing also changes the rendered world camera.
    pub const fn camera_enabled(self) -> bool {
        matches!(self, Self::Full)
    }
}
