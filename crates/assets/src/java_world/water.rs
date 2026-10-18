//! Animation records for locally loaded water art.

use super::JavaTexture;
use serde_json::Value;
use std::collections::BTreeMap;

/// One vertical flipbook with its authored tick interval.
#[derive(Clone, Debug)]
pub struct JavaWaterTexture {
    pub image: JavaTexture,
    pub frame_ticks: u32,
}

impl JavaWaterTexture {
    /// Returns the number of square frames admitted during decoding.
    pub fn frame_count(&self) -> u32 {
        self.image.height / self.image.width
    }
}

pub const WATER_TEXTURE_PATHS: [&str; 2] = [
    "assets/minecraft/textures/block/water_still.png",
    "assets/minecraft/textures/block/water_flow.png",
];

/// Reads bounded regular flipbooks and rejects metadata this renderer cannot reproduce.
pub(super) fn decode(
    textures: &BTreeMap<Box<str>, JavaTexture>,
    metadata: &BTreeMap<String, Value>,
) -> Result<[JavaWaterTexture; WATER_TEXTURE_PATHS.len()], String> {
    let mut decoded = Vec::new();
    for path in WATER_TEXTURE_PATHS {
        let image = textures.get(path).ok_or("missing Java water image")?;
        if !image.width.is_power_of_two()
            || image.width > 64
            || image.height % image.width != 0
            || image.height / image.width > 256
        {
            return Err("unsupported Java water flipbook dimensions".into());
        }
        let animation = metadata
            .get(&format!("{path}.mcmeta"))
            .and_then(|value| value.get("animation"))
            .ok_or("missing Java water animation")?;
        if animation.get("frames").is_some()
            || animation
                .get("interpolate")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            return Err("unsupported Java water frame sequence".into());
        }
        let ticks = animation
            .get("frametime")
            .map_or(Some(1), Value::as_u64)
            .filter(|ticks| (1..=1200).contains(ticks))
            .ok_or("invalid Java water frame time")?;
        decoded.push(JavaWaterTexture {
            image: image.clone(),
            frame_ticks: ticks as u32,
        });
    }
    decoded
        .try_into()
        .map_err(|_| "Java water texture count mismatch".into())
}
