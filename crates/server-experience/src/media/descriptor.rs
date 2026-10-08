//! A media descriptor is an indexed file covered by the bundle publisher signature.

use super::*;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub id: String,
    pub timeline: String,
    pub profile: Profile,
    pub url: String,
    pub bytes: u64,
    pub chunk_bytes: u32,
    pub chunk_hashes: Vec<String>,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_us: u64,
    pub audio_channels: u8,
    pub poster: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    WebmAv1OpusBt709,
}

impl Descriptor {
    /// Checks the complete signed chunk index and declared decoder allocation budget.
    pub fn validate(&self, origins: &BTreeSet<String>) -> Result<()> {
        self.validate_profile()?;
        crate::fetch::approved_url(&self.url, origins)?;
        Ok(())
    }

    /// Everything but origin approval, which only the fetching parent can check.
    pub fn validate_profile(&self) -> Result<()> {
        ensure!(
            crate::manifest::identifier(&self.id) && crate::manifest::identifier(&self.timeline),
            "invalid media identity"
        );
        ensure!(
            self.width > 0
                && self.width <= MAX_WIDTH
                && self.width.is_multiple_of(2)
                && self.height > 0
                && self.height <= MAX_HEIGHT
                && self.height.is_multiple_of(2),
            "unsupported dimensions"
        );
        ensure!(
            self.fps > 0
                && self.fps <= MAX_FPS
                && self.duration_us > 0
                && self.duration_us <= MAX_DURATION_US,
            "unsupported media timing"
        );
        ensure!(
            (1..=2).contains(&self.audio_channels),
            "unsupported audio layout"
        );
        ensure!(
            (64 * 1024..=1024 * 1024).contains(&self.chunk_bytes),
            "invalid chunk size"
        );
        ensure!(
            self.bytes > 0
                && self.chunk_hashes.len() <= 4096
                && self.bytes.div_ceil(u64::from(self.chunk_bytes))
                    == self.chunk_hashes.len() as u64,
            "invalid integrity index"
        );
        crate::crypto::fixed_hex::<32>(&self.sha256)?;
        for hash in &self.chunk_hashes {
            crate::crypto::fixed_hex::<32>(hash)?;
        }
        Ok(())
    }
}
