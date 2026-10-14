//! Video presets share one table with application and derived selection.

use super::{
    DISTANT_TERRAIN_DISTANCE_OPTION, DISTANT_TERRAIN_OPTION, DISTANT_TERRAIN_QUALITY_OPTION,
    FRAME_RATE_UNLIMITED, SETTINGS_OPTIONS, SMAA_OPTION, SettingsOptions,
};

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
    pub fn controls(name: &str) -> bool {
        PRESET_OPTIONS.iter().any(|option| option.name == name)
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
    name: &'static str,
    values: [Value; VideoPreset::ALL.len()],
}

use Value::{Default, Fixed, HighestSamples, PerformanceDistance, QualityDistance};

// Each row supplies every preset. Balanced reads existing defaults, including device limits.
const PRESET_OPTIONS: &[PresetOption] = &[
    PresetOption {
        name: "render_distance",
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
        name: "msaa",
        values: [
            Fixed(ui::ANTI_ALIASING_SAMPLE_COUNTS[0] as i32),
            Default,
            HighestSamples,
        ],
    },
    PresetOption {
        name: SMAA_OPTION.name,
        values: [
            Fixed(ui::SmaaMode::Off as i32),
            Default,
            Fixed(ui::SmaaMode::Smaa as i32),
        ],
    },
    PresetOption {
        name: "max_framerate",
        values: [
            Fixed(FRAME_RATE_UNLIMITED),
            Default,
            Fixed(FRAME_RATE_UNLIMITED),
        ],
    },
    PresetOption {
        name: "vsync",
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        name: "graphics_mode",
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        name: "smooth_lighting",
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        name: "fancy_skies",
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        name: "render_clouds",
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        name: "transparent_leaves",
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        name: "bubble_particles",
        values: [Fixed(0), Default, Fixed(1)],
    },
    // Distant terrain: Balanced is the defaults, which keep it off; Quality draws the tier's
    // farthest distance at Medium.
    PresetOption {
        name: DISTANT_TERRAIN_OPTION.name,
        values: [Fixed(0), Default, Fixed(1)],
    },
    PresetOption {
        name: DISTANT_TERRAIN_DISTANCE_OPTION.name,
        values: [
            Default,
            Default,
            Fixed(render_api::DistantTerrainTier::CURRENT.max_distance_chunks as i32),
        ],
    },
    PresetOption {
        name: DISTANT_TERRAIN_QUALITY_OPTION.name,
        values: [
            Default,
            Default,
            Fixed(render_api::DistantTerrainQuality::Medium.index()),
        ],
    },
];

impl PresetOption {
    /// Finds the owning registry entry; the table coverage test guards this contract.
    fn index(&self) -> usize {
        SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == self.name)
            .expect("preset option must be registered")
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
        if self.name == "msaa" {
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
                .all(|option| self.value(option.name) == option.value(self, *preset))
        })
    }

    /// Applies only preset options. Balanced removes their overrides to restore live defaults.
    pub fn apply_video_preset(&mut self, preset: VideoPreset) -> bool {
        let mut changed = false;
        for option in PRESET_OPTIONS {
            if preset == VideoPreset::Balanced {
                changed |= self.values.remove(option.name).is_some();
            } else {
                changed |= self.set(option.index(), option.value(self, preset));
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests;
