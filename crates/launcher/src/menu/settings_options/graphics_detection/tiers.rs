//! Admission thresholds for the existing Video presets, ordered from low to high.

use super::VideoPreset;

/// A row bounds RAM, core counts and the selected GPU's reported memory.
#[derive(serde::Serialize)]
pub struct Tier {
    pub preset: VideoPreset,
    /// Uses the shared resource-pack memory scale rather than a second RAM scale.
    pub memory_tier: u32,
    pub logical_cores: u32,
    pub physical_cores: u32,
    pub discrete_memory_bytes: u64,
    pub integrated_memory_bytes: u64,
}

/// Integrated GPUs need a larger shared allocation for Quality than dedicated GPUs.
/// These defaults are hardware heuristics, not a frame-rate guarantee.
pub const TIERS: [Tier; VideoPreset::ALL.len()] = [
    Tier {
        preset: VideoPreset::Performance,
        memory_tier: 0,
        logical_cores: 0,
        physical_cores: 0,
        discrete_memory_bytes: 0,
        integrated_memory_bytes: 0,
    },
    Tier {
        preset: VideoPreset::Balanced,
        memory_tier: 3,
        logical_cores: 4,
        physical_cores: 4,
        discrete_memory_bytes: 2 << 30,
        integrated_memory_bytes: 2 << 30,
    },
    Tier {
        preset: VideoPreset::Quality,
        memory_tier: 5,
        logical_cores: 8,
        physical_cores: 8,
        discrete_memory_bytes: 4 << 30,
        integrated_memory_bytes: 8 << 30,
    },
];
