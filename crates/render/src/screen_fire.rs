//! Native camera fire uses the fire block's destruction (down-face) sprite on an open cube.
use assets::{ANIMATION_FLAG_BLEND, BlockFace, NetworkIdMode, RuntimeAssets, TextureRef};

use crate::ChunkAnimationClock;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct ScreenFireTexture {
    pub(crate) side: u32,
    pub(crate) pixels: Vec<u8>,
    pub(crate) frames: u32,
    timeline: Vec<u32>,
    ticks_per_frame: u32,
    blend: bool,
}

impl ScreenFireTexture {
    /// Copies the admitted carrier's animation timeline, including server pack overrides.
    pub fn from_assets(assets: &RuntimeAssets, fire_state: u32) -> Option<Self> {
        if assets.is_diagnostic() {
            return None;
        }
        let block = assets.resolve(NetworkIdMode::Sequential, fire_state);
        if !block.is_known() {
            return None;
        }
        let material = assets.material(block.face(BlockFace::Down).material_id());
        let animation = assets.animations().get(material.animation as usize);
        let static_frame = [material.texture];
        let frames = match animation {
            Some(animation) => assets.animation_frames().get(
                animation.frame_start as usize
                    ..animation.frame_start.checked_add(animation.frame_count)? as usize,
            )?,
            None => &static_frame,
        };
        let first = pixels(assets, *frames.first()?)?;
        let side = first.0;
        let mut rgba8 = Vec::new();
        let mut layers = BTreeMap::new();
        let mut timeline = Vec::with_capacity(frames.len());
        for &frame in frames {
            if let Some(&layer) = layers.get(&frame.raw()) {
                timeline.push(layer);
                continue;
            }
            let (frame_side, bytes) = pixels(assets, frame)?;
            if frame_side != side {
                return None;
            }
            let layer = u32::try_from(layers.len()).ok()?;
            rgba8.extend_from_slice(bytes);
            layers.insert(frame.raw(), layer);
            timeline.push(layer);
        }
        Some(Self {
            side,
            pixels: rgba8,
            frames: layers.len().try_into().ok()?,
            timeline,
            ticks_per_frame: animation.map_or(1, |animation| animation.ticks_per_frame),
            blend: animation.is_some_and(|animation| animation.flags & ANIMATION_FLAG_BLEND != 0),
        })
    }

    pub(crate) fn sample(&self, clock: ChunkAnimationClock) -> [f32; 4] {
        let current = (clock.tick() / self.ticks_per_frame) as usize % self.timeline.len();
        let next = (current + 1) % self.timeline.len();
        let blend = if self.blend {
            (clock.tick() % self.ticks_per_frame) as f32 + clock.partial_tick()
        } else {
            0.0
        } / self.ticks_per_frame as f32;
        [
            self.timeline[current] as f32,
            self.timeline[next] as f32,
            blend,
            1.0,
        ]
    }
}

fn pixels(assets: &RuntimeAssets, frame: TextureRef) -> Option<(u32, &[u8])> {
    let texture = &assets.texture_pages().get(frame.page() as usize)?.texture;
    let mip = texture.mips.first()?;
    let bytes = usize::try_from(mip.size.checked_mul(mip.size)?.checked_mul(4)?).ok()?;
    let start = (frame.layer() as usize).checked_mul(bytes)?;
    Some((mip.size, mip.rgba8.get(start..start.checked_add(bytes)?)?))
}

#[cfg(test)]
#[path = "screen_fire/tests.rs"]
mod tests;
