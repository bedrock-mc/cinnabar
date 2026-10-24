//! Persisted VSync choices.

/// VSync values retain the original saved Off (0) and On (1) choices.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[repr(i32)]
pub enum VsyncMode {
    /// Uncapped presentation that may tear: Immediate, then Mailbox, then FIFO.
    Off = 0,
    /// Display-paced FIFO presentation.
    On = 1,
    /// Unpaced but tear-free where possible: Mailbox, then Immediate, then FIFO.
    OffNoTearing = 2,
}

impl VsyncMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::On, Self::OffNoTearing];
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
            2 => Self::OffNoTearing,
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
            Self::OffNoTearing => "Off (no tearing)",
        }
    }

    /// Stable radio binding shared by both settings interfaces.
    #[must_use]
    pub const fn binding(self) -> &'static str {
        match self {
            Self::Off => "vsync_radio_off",
            Self::On => "vsync_radio_on",
            Self::OffNoTearing => "vsync_radio_no_tearing",
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
