//! Hardware recommendations never take ownership of manually edited settings.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{SETTINGS_OPTIONS, SettingsOptions, video_presets::VideoPreset};

mod tiers;
pub use tiers::TIERS;

pub const TUNED_MESSAGE: &str = "Graphics defaults detected — your edits are kept.";
const CACHE_CAPACITY: usize = 4;

/// The selected adapter's memory and execution model.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum DeviceType {
    Integrated,
    Discrete,
    Virtual,
    Cpu,
    #[default]
    Unknown,
}

/// Adapter and driver facts that identify a cached hardware recommendation.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AdapterIdentity {
    pub vendor: u32,
    pub device: u32,
    pub name: String,
    pub device_type: DeviceType,
    pub backend: String,
    pub driver: String,
    pub driver_info: String,
    pub driver_build: String,
}

impl AdapterIdentity {
    /// Hashes the adapter, driver and current thresholds so policy changes invalidate the cache.
    pub fn cache_key(&self) -> String {
        let identity = serde_json::to_vec(&(self, TIERS))
            .expect("adapter identity contains only serializable values");
        format!("{:x}", Sha256::digest(identity))
    }
}

/// Installed hardware read once at startup; zero counts and absent memory mean unknown.
#[derive(Clone, Debug, Default)]
pub struct HardwareFacts {
    pub adapter: AdapterIdentity,
    /// Dedicated VRAM or the platform-reported integrated GPU memory share.
    pub graphics_memory_bytes: Option<u64>,
    pub physical_memory_bytes: u64,
    pub logical_cores: u32,
    pub physical_cores: u32,
}

impl HardwareFacts {
    /// Selects the highest tier admitted by the installed CPU, GPU and memory.
    pub fn tier(&self) -> VideoPreset {
        match self.adapter.device_type {
            DeviceType::Cpu | DeviceType::Virtual => return VideoPreset::Performance,
            DeviceType::Unknown => return VideoPreset::Balanced,
            DeviceType::Integrated | DeviceType::Discrete => {}
        }
        let Some(graphics_bytes) = self.graphics_memory_bytes.filter(|bytes| *bytes > 0) else {
            return VideoPreset::Balanced;
        };
        if self.physical_memory_bytes == 0 || self.logical_cores == 0 || self.physical_cores == 0 {
            return VideoPreset::Balanced;
        }
        let memory_tier = resource_pack::subpacks::device_memory_tier(self.physical_memory_bytes);
        TIERS
            .iter()
            .rev()
            .find(|tier| {
                let minimum_graphics = match self.adapter.device_type {
                    DeviceType::Integrated => tier.integrated_memory_bytes,
                    _ => tier.discrete_memory_bytes,
                };
                graphics_bytes >= minimum_graphics
                    && memory_tier >= tier.memory_tier
                    && self.logical_cores >= tier.logical_cores
                    && self.physical_cores >= tier.physical_cores
            })
            .expect("the Performance tier admits all known hardware")
            .preset
    }
}

/// A preset with distance bounded by the existing device recommendation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Recommendation {
    pub preset: VideoPreset,
    pub render_distance: u8,
}

/// Applies the hardware preset with the existing device distance limits.
pub fn recommend(settings: &SettingsOptions, preset: VideoPreset) -> Recommendation {
    let defaults = settings.render_distance_defaults();
    let distance = settings
        .video_preset_values(preset)
        .find(|(name, _)| *name == "render_distance")
        .expect("presets include distance")
        .1 as u8;
    Recommendation {
        preset,
        render_distance: distance
            .min(defaults.recommended)
            .min(defaults.native_maximum),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct CachedTier {
    key: String,
    preset: VideoPreset,
}

/// Persisted defaults, protected choices and a bounded cache of hardware tiers.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    automatic: BTreeMap<String, i32>,
    protected: BTreeSet<String>,
    // Old measurement caches are ignored without discarding saved user settings.
    #[serde(rename = "hardware_cache")]
    cache: Vec<CachedTier>,
}

/// Accepts only the hexadecimal digest used by adapter identities.
fn valid_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Preferences {
    /// Reads automatic defaults only while the user has not claimed the option.
    pub(super) fn value(&self, name: &str) -> Option<i32> {
        (!self.protected.contains(name))
            .then(|| self.automatic.get(name).copied())
            .flatten()
    }

    /// Remembers explicit edits even when their value equals the ordinary default.
    pub(super) fn protect(&mut self, name: &str) -> bool {
        VideoPreset::controls(name) && self.protected.insert(name.to_owned())
    }

    /// Rejects unknown options, invalid values and oversized cache records.
    pub(super) fn validated(mut self) -> Self {
        self.automatic.retain(|name, value| {
            VideoPreset::controls(name)
                && SETTINGS_OPTIONS.iter().any(|option| {
                    option.name == name
                        && (option.min..=option.max).contains(value)
                        && (*value - option.min) % option.step == 0
                })
        });
        self.protected.retain(|name| VideoPreset::controls(name));
        self.cache.retain(|entry| valid_key(&entry.key));
        self.cache.truncate(CACHE_CAPACITY);
        self
    }
}

impl SettingsOptions {
    /// Returns a tier only for the same adapter, driver and hardware thresholds.
    pub fn graphics_tier(&self, key: &str) -> Option<VideoPreset> {
        self.graphics_detection
            .cache
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.preset)
    }

    /// Saves a hardware tier, keeping at most four adapter identities.
    pub fn cache_graphics_tier(&mut self, key: String, preset: VideoPreset) -> bool {
        if !valid_key(&key) {
            return false;
        }
        self.graphics_detection
            .cache
            .retain(|entry| entry.key != key);
        self.graphics_detection
            .cache
            .insert(0, CachedTier { key, preset });
        self.graphics_detection.cache.truncate(CACHE_CAPACITY);
        true
    }

    /// Refreshes untouched settings through the existing preset table; user values win.
    pub fn apply_graphics_recommendation(&mut self, recommendation: Recommendation) -> bool {
        let automatic = self
            .video_preset_values(recommendation.preset)
            .map(|(name, value)| {
                (
                    name.to_owned(),
                    if name == "render_distance" {
                        i32::from(recommendation.render_distance)
                    } else {
                        value
                    },
                )
            })
            .collect();
        if self.graphics_detection.automatic == automatic {
            return false;
        }
        self.graphics_detection.automatic = automatic;
        true
    }
}

#[cfg(test)]
mod tests;
