//! Video presets share one table with application and derived selection.

use super::{FRAME_RATE_UNLIMITED, SETTINGS_OPTIONS, SettingId, SettingsOptions};

/// Named graphics choices; a selection is Custom when none matches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoPreset {
    Performance,
    Balanced,
    Quality,
}

impl VideoPreset {
    pub const ALL: [Self; 3] = [Self::Performance, Self::Balanced, Self::Quality];

    /// The caption shared by preset controls and their current selection.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Balanced => "Balanced",
            Self::Quality => "Quality",
        }
    }

    /// Selects the corresponding column in the preset table.
    const fn index(self) -> usize {
        match self {
            Self::Performance => 0,
            Self::Balanced => 1,
            Self::Quality => 2,
        }
    }

    /// Identifies real edits that replace a temporary override of a preset option.
    pub fn controls(id: SettingId) -> bool {
        PRESET_OPTIONS.iter().any(|option| option.id == id)
    }
}

#[derive(Clone, Copy)]
enum Value {
    Default,
    Fixed(i32),
    PerformanceDistance(u8),
    QualityDistance { minimum: u8, maximum: u8 },
    HighestSamples,
}

struct PresetOption {
    id: SettingId,
    values: [Value; VideoPreset::ALL.len()],
}

use Value::{Default, Fixed, HighestSamples, PerformanceDistance, QualityDistance};

// Each row supplies every preset. Balanced reads existing defaults, including device limits.
// VSync stays a separate player preference when applying or matching graphics presets.
const PRESET_OPTIONS: &[PresetOption] = &[
    PresetOption {
        id: SettingId::VerticalRenderDistance,
        values: [Default; VideoPreset::ALL.len()],
    },
    PresetOption {
        id: SettingId::RenderDistance,
        values: [
            PerformanceDistance(12),
            Default,
            QualityDistance {
                minimum: 32,
                maximum: 64,
            },
        ],
    },
    PresetOption {
        id: SettingId::Msaa,
        values: [
            Fixed(ui::ANTI_ALIASING_SAMPLE_COUNTS[0] as i32),
            Default,
            HighestSamples,
        ],
    },
    PresetOption {
        id: SettingId::Smaa,
        values: [
            Fixed(ui::SmaaMode::Off as i32),
            Default,
            Fixed(ui::SmaaMode::Smaa as i32),
        ],
    },
    PresetOption {
        id: SettingId::MaxFramerate,
        values: [
            Fixed(FRAME_RATE_UNLIMITED),
            Default,
            Fixed(FRAME_RATE_UNLIMITED),
        ],
    },
    PresetOption {
        id: SettingId::GraphicsMode,
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        id: SettingId::SmoothLighting,
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        id: SettingId::FancySkies,
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        id: SettingId::RenderClouds,
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        id: SettingId::TransparentLeaves,
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        id: SettingId::BubbleParticles,
        values: [Fixed(0), Default, Fixed(1)],
    },
];

impl PresetOption {
    /// Finds the owning registry entry; the table coverage test guards this contract.
    fn index(&self) -> usize {
        self.id.index()
    }

    /// Resolves a table cell against the same defaults and capabilities as the controls.
    fn value(&self, settings: &SettingsOptions, preset: VideoPreset) -> i32 {
        let value = match self.values[preset.index()] {
            Default => settings.option_default(&SETTINGS_OPTIONS[self.index()]),
            Fixed(value) => value,
            PerformanceDistance(maximum) => {
                settings.render_distance_defaults().recommended.min(maximum) as i32
            }
            QualityDistance { minimum, maximum } => {
                let defaults = settings.render_distance_defaults();
                defaults
                    .recommended
                    .max(minimum)
                    .min(maximum)
                    .min(defaults.native_maximum) as i32
            }
            HighestSamples => settings
                .anti_aliasing_support()
                .counts()
                .next_back()
                .unwrap_or(1) as i32,
        };
        if self.id == SettingId::Msaa {
            settings.anti_aliasing_support().select(value as u32) as i32
        } else {
            value
        }
    }
}

impl SettingsOptions {
    /// Derives the selected preset from effective option values; None means Custom.
    pub fn video_preset(&self) -> Option<VideoPreset> {
        VideoPreset::ALL.into_iter().find(|preset| {
            PRESET_OPTIONS
                .iter()
                .all(|option| self.value(option.id) == option.value(self, *preset))
        })
    }

    /// Applies only preset options. Balanced removes their overrides to restore live defaults.
    pub fn apply_video_preset(&mut self, preset: VideoPreset) -> bool {
        let mut changed = false;
        for option in PRESET_OPTIONS {
            if preset == VideoPreset::Balanced {
                changed |= self.values.remove(option.id.name()).is_some();
            } else {
                changed |= self.set(option.index(), option.value(self, preset));
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests;
