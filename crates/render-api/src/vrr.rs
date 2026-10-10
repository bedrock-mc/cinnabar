/// How the player wants variable-refresh timing to be determined.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
#[repr(i32)]
pub enum VrrPreference {
    #[default]
    Automatic = 0,
    /// The player declares VRR enabled; this does not reconfigure the OS or display.
    On = 1,
    /// Ignores platform VRR reports when choosing the frame-rate cap.
    Off = 2,
}

impl VrrPreference {
    /// Decodes saved choices, leaving unknown values on automatic detection.
    pub const fn from_value(value: i32) -> Self {
        match value {
            1 => Self::On,
            2 => Self::Off,
            _ => Self::Automatic,
        }
    }

    /// The caption shared by the Video settings hosts.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::On => "On",
            Self::Off => "Off",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vrr_choices_round_trip_and_unknown_values_stay_automatic() {
        for choice in [
            VrrPreference::Automatic,
            VrrPreference::On,
            VrrPreference::Off,
        ] {
            assert_eq!(VrrPreference::from_value(choice as i32), choice);
            assert!(!choice.label().is_empty());
        }
        for invalid in [-1, 3, i32::MAX] {
            assert_eq!(VrrPreference::from_value(invalid), VrrPreference::Automatic);
        }
    }
}
