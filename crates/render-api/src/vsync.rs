//! Persisted VSync choices, including an explicit opt-in to tearing.

/// VSync values retain the original saved Off (0) and On (1) choices.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(i32)]
pub enum VsyncMode {
    Off = 0,
    On = 1,
    AllowTearing = 2,
}

impl VsyncMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::On, Self::AllowTearing];
    /// Mobile devices keep synchronized presentation; desktop starts with low latency.
    pub const DEFAULT: Self = if cfg!(target_os = "android") {
        Self::On
    } else {
        Self::Off
    };

    /// Decodes the persisted choice, retaining the platform default for invalid values.
    #[must_use]
    pub const fn from_value(value: i32) -> Self {
        match value {
            0 => Self::Off,
            1 => Self::On,
            2 => Self::AllowTearing,
            _ => Self::DEFAULT,
        }
    }

    /// Whether the player requested synchronized presentation.
    #[must_use]
    pub const fn enabled(self) -> bool {
        matches!(self, Self::On)
    }

    /// Caption shared by both settings interfaces.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
            Self::AllowTearing => "Off (allow tearing)",
        }
    }

    /// Stable radio binding shared by both settings interfaces.
    #[must_use]
    pub const fn binding(self) -> &'static str {
        match self {
            Self::Off => "vsync_radio_off",
            Self::On => "vsync_radio_on",
            Self::AllowTearing => "vsync_radio_tearing",
        }
    }
}

impl Default for VsyncMode {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<bool> for VsyncMode {
    fn from(enabled: bool) -> Self {
        if enabled { Self::On } else { Self::Off }
    }
}
