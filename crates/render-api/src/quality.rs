//! Saved quality choices for the optional Enhanced renderer.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EnhancedQuality {
    Performance,
    #[default]
    Balanced,
    Ultra,
}

impl EnhancedQuality {
    pub const ALL: [Self; 3] = [Self::Performance, Self::Balanced, Self::Ultra];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Balanced => "balanced",
            Self::Ultra => "ultra",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Balanced => "Balanced",
            Self::Ultra => "Ultra",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|quality| value.trim().eq_ignore_ascii_case(quality.as_str()))
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Performance => Self::Balanced,
            Self::Balanced => Self::Ultra,
            Self::Ultra => Self::Performance,
        }
    }
}
